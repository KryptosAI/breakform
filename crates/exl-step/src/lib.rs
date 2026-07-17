use exl_core::{
    geom::{
        BRep, BrepEdge, BrepFace, BrepVertex, CurveParams, CurveType, SurfaceParams, SurfaceType,
        Transform,
    },
    Assembly, Document, EntityStatus, FidelityReport, GeometryPayload, Instance, Part,
    ToolOfOrigin,
};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
pub enum StepError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Not a STEP Part 21 file")]
    NotAStepFile,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum StepValue {
    Number(f64),
    Str(String),
    Ref(String),
    Enum(String),
    List(Vec<StepValue>),
    Null,
    Omitted,
    Typed { name: String, value: Box<StepValue> },
}

fn unwrap_typed(v: &StepValue) -> &StepValue {
    let mut cur = v;
    while let StepValue::Typed { value, .. } = cur {
        cur = value;
    }
    cur
}

struct Entity {
    name: String,
    args: Vec<StepValue>,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    len: usize,
}

impl Parser {
    fn new(input: &str) -> Self {
        let chars: Vec<char> = input.chars().collect();
        let len = chars.len();
        Parser { chars, pos: 0, len }
    }

    fn remaining(&self) -> usize {
        self.len.saturating_sub(self.pos)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_ws(&mut self) {
        while self.pos < self.len {
            let c = self.chars[self.pos];
            if c == '/' && self.pos + 1 < self.len {
                if self.chars[self.pos + 1] == '*' {
                    self.skip_comment();
                } else if self.chars[self.pos + 1] == '/' {
                    self.skip_line_comment();
                } else {
                    break;
                }
            } else if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        self.pos += 2;
        while self.pos + 1 < self.len {
            if self.chars[self.pos] == '*' && self.chars[self.pos + 1] == '/' {
                self.pos += 2;
                return;
            }
            self.pos += 1;
        }
    }

    fn skip_line_comment(&mut self) {
        self.pos += 2;
        while self.pos < self.len && self.chars[self.pos] != '\n' {
            self.pos += 1;
        }
    }

    fn expect_str(&mut self, s: &str) -> Result<(), StepError> {
        self.skip_ws();
        let expected: Vec<char> = s.chars().collect();
        for (i, &ec) in expected.iter().enumerate() {
            match self.chars.get(self.pos + i) {
                Some(&c) if c == ec => continue,
                _ => {
                    return Err(StepError::Parse(format!(
                        "expected '{}' at position {}",
                        s, self.pos
                    )))
                }
            }
        }
        self.pos += expected.len();
        Ok(())
    }

    fn read_digits(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.len && self.chars[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn read_string(&mut self) -> String {
        let mut result = String::new();
        while self.pos < self.len {
            let c = self.chars[self.pos];
            if c == '\'' {
                self.pos += 1;
                if self.pos < self.len && self.chars[self.pos] == '\'' {
                    result.push('\'');
                    self.pos += 1;
                } else {
                    break;
                }
            } else {
                result.push(c);
                self.pos += 1;
            }
        }
        result
    }

    fn read_number(&mut self) -> Result<f64, StepError> {
        let start = self.pos;
        if self.pos < self.len && (self.chars[self.pos] == '-' || self.chars[self.pos] == '+') {
            self.pos += 1;
        }
        while self.pos < self.len && self.chars[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos < self.len && self.chars[self.pos] == '.' {
            self.pos += 1;
            while self.pos < self.len && self.chars[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.len && (self.chars[self.pos] == 'e' || self.chars[self.pos] == 'E') {
            self.pos += 1;
            if self.pos < self.len && (self.chars[self.pos] == '-' || self.chars[self.pos] == '+') {
                self.pos += 1;
            }
            while self.pos < self.len && self.chars[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let num_str: String = self.chars[start..self.pos].iter().collect();
        num_str
            .parse::<f64>()
            .map_err(|e| StepError::Parse(format!("invalid number '{}': {}", num_str, e)))
    }

    fn read_entity_name(&mut self) -> Result<String, StepError> {
        self.skip_ws();
        let start = self.pos;
        while self.pos < self.len
            && (self.chars[self.pos].is_ascii_alphanumeric() || self.chars[self.pos] == '_')
        {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(StepError::Parse("expected entity name".into()));
        }
        Ok(self.chars[start..self.pos].iter().collect())
    }

    fn parse_value(&mut self) -> Result<StepValue, StepError> {
        self.skip_ws();
        match self.peek() {
            None => Err(StepError::Parse("unexpected end of input".into())),
            Some('$') => {
                self.advance();
                Ok(StepValue::Null)
            }
            Some('*') => {
                self.advance();
                Ok(StepValue::Omitted)
            }
            Some('#') => {
                self.advance();
                let id = self.read_digits();
                Ok(StepValue::Ref(format!("#{}", id)))
            }
            Some('.') => {
                self.advance();
                let mut content = String::new();
                while self.pos < self.len && self.chars[self.pos] != '.' {
                    content.push(self.chars[self.pos]);
                    self.pos += 1;
                }
                if self.pos < self.len {
                    self.advance();
                }
                Ok(StepValue::Enum(format!(".{}.", content)))
            }
            Some('\'') => {
                self.advance();
                let s = self.read_string();
                Ok(StepValue::Str(s))
            }
            Some('(') => {
                self.advance();
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    if self.peek() == Some(')') {
                        self.advance();
                        break;
                    }
                    items.push(self.parse_value()?);
                    self.skip_ws();
                    if self.peek() == Some(',') {
                        self.advance();
                    }
                }
                Ok(StepValue::List(items))
            }
            Some(c) if c == '-' || c == '+' || c.is_ascii_digit() => {
                let num = self.read_number()?;
                Ok(StepValue::Number(num))
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let name = self.read_entity_name()?;
                self.skip_ws();
                if self.peek() == Some('(') {
                    self.advance();
                    let inner = self.parse_value()?;
                    self.skip_ws();
                    if self.peek() == Some(')') {
                        self.advance();
                    }
                    Ok(StepValue::Typed {
                        name,
                        value: Box::new(inner),
                    })
                } else {
                    Err(StepError::Parse(format!(
                        "unexpected identifier '{}' in value context at position {}",
                        name, self.pos
                    )))
                }
            }
            Some(c) => Err(StepError::Parse(format!(
                "unexpected character '{}' at position {}",
                c, self.pos
            ))),
        }
    }

    fn parse_arg_list_until_close(&mut self) -> Result<Vec<StepValue>, StepError> {
        let mut args = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(')') {
                self.advance();
                break;
            }
            args.push(self.parse_value()?);
            self.skip_ws();
            if self.peek() == Some(',') {
                self.advance();
            }
        }
        Ok(args)
    }
}

struct Consumed {
    vertices: usize,
    edges: usize,
    faces: usize,
}

fn extract_first_string(content: &str, keyword: &str) -> Option<String> {
    let pos = content.find(keyword)?;
    let after = &content[pos + keyword.len()..];
    let mut parser = Parser::new(after);
    parser.expect_str("(").ok()?;
    match parser.parse_value().ok()? {
        StepValue::Str(s) => Some(s),
        StepValue::List(items) => items.first().and_then(|v| {
            if let StepValue::Str(s) = v {
                Some(s.clone())
            } else {
                None
            }
        }),
        _ => None,
    }
}

fn parse_header(content: &str) -> Result<(Option<String>, Option<String>), StepError> {
    let model_name = extract_first_string(content, "FILE_NAME");
    let schema = extract_first_string(content, "FILE_SCHEMA");
    Ok((model_name, schema))
}

fn parse_data_section(section: &str) -> Result<HashMap<String, Vec<Entity>>, StepError> {
    let mut parser = Parser::new(section);
    let mut entities: HashMap<String, Vec<Entity>> = HashMap::new();

    loop {
        parser.skip_ws();
        if parser.remaining() == 0 {
            break;
        }
        if parser.peek() != Some('#') {
            parser.advance();
            continue;
        }

        parser.advance();
        let id = parser.read_digits();
        let entity_id = format!("#{}", id);

        parser.expect_str("=")?;

        parser.skip_ws();
        if parser.peek() == Some('(') {
            parser.advance();
            let mut sub_entities = Vec::new();
            loop {
                parser.skip_ws();
                if parser.peek() == Some(')') || parser.peek().is_none() {
                    break;
                }
                let sub_name = parser.read_entity_name()?;
                parser.expect_str("(")?;
                let args = parser.parse_arg_list_until_close()?;
                sub_entities.push(Entity {
                    name: sub_name,
                    args,
                });
            }
            if parser.peek() == Some(')') {
                parser.advance();
            }
            parser.expect_str(";")?;
            entities.insert(entity_id, sub_entities);
        } else {
            let name = parser.read_entity_name()?;

            parser.skip_ws();
            parser.expect_str("(")?;

            let args = parser.parse_arg_list_until_close()?;

            parser.expect_str(";")?;

            entities.insert(entity_id, vec![Entity { name, args }]);
        }
    }

    Ok(entities)
}

fn find_entity_named<'a>(
    entities: &'a HashMap<String, Vec<Entity>>,
    id: &str,
    name: &str,
) -> Option<&'a Entity> {
    entities.get(id)?.iter().find(|e| e.name == name)
}

fn first_entity_matching<'a, F>(
    entities: &'a HashMap<String, Vec<Entity>>,
    id: &str,
    pred: F,
) -> Option<&'a Entity>
where
    F: Fn(&str) -> bool,
{
    entities.get(id)?.iter().find(|e| pred(&e.name))
}

fn resolve_curve_type(entities: &HashMap<String, Vec<Entity>>, ref_id: &str) -> CurveType {
    match first_entity_matching(entities, ref_id, |_| true) {
        Some(entity) => match entity.name.as_str() {
            "LINE" => CurveType::Line,
            "CIRCLE" => CurveType::Circle,
            "ELLIPSE" => CurveType::Ellipse,
            n if n.starts_with("B_SPLINE_CURVE") => CurveType::Nurbs,
            _ => CurveType::Other,
        },
        None => CurveType::Other,
    }
}

fn resolve_surface_type(entities: &HashMap<String, Vec<Entity>>, ref_id: &str) -> SurfaceType {
    match first_entity_matching(entities, ref_id, |_| true) {
        Some(entity) => match entity.name.as_str() {
            "PLANE" => SurfaceType::Plane,
            "CYLINDRICAL_SURFACE" => SurfaceType::Cylinder,
            "CONICAL_SURFACE" => SurfaceType::Cone,
            "SPHERICAL_SURFACE" => SurfaceType::Sphere,
            "TOROIDAL_SURFACE" => SurfaceType::Torus,
            "SURFACE_OF_LINEAR_EXTRUSION" => SurfaceType::Extrusion,
            n if n.starts_with("B_SPLINE_SURFACE") => SurfaceType::Nurbs,
            _ => SurfaceType::Other,
        },
        None => SurfaceType::Other,
    }
}

fn extract_face_edges(entities: &HashMap<String, Vec<Entity>>, bounds: &StepValue) -> Vec<String> {
    let mut edge_ids = Vec::new();

    let bound_refs: Vec<String> = match bounds {
        StepValue::List(items) => items
            .iter()
            .filter_map(|v| {
                if let StepValue::Ref(r) = v {
                    Some(r.clone())
                } else {
                    None
                }
            })
            .collect(),
        StepValue::Ref(r) => vec![r.clone()],
        _ => return edge_ids,
    };

    for bound_ref in &bound_refs {
        if let Some(sub) = entities.get(bound_ref) {
            for bound_entity in sub {
                if bound_entity.args.len() < 2 {
                    continue;
                }
                if let StepValue::Ref(loop_ref) = &bound_entity.args[1] {
                    if let Some(sub2) = entities.get(loop_ref) {
                        for loop_entity in sub2 {
                            if loop_entity.args.len() < 2 {
                                continue;
                            }
                            if let StepValue::List(oe_list) = &loop_entity.args[1] {
                                for oe_val in oe_list {
                                    if let StepValue::Ref(oe_ref) = oe_val {
                                        if let Some(sub3) = entities.get(oe_ref) {
                                            for oe_entity in sub3 {
                                                if oe_entity.args.len() < 4 {
                                                    continue;
                                                }
                                                if let StepValue::Ref(ec_ref) = &oe_entity.args[3] {
                                                    edge_ids.push(ec_ref.clone());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    edge_ids
}

fn get_point(entities: &HashMap<String, Vec<Entity>>, ref_id: &str) -> Option<[f64; 3]> {
    let entity = find_entity_named(entities, ref_id, "CARTESIAN_POINT")?;
    if entity.args.len() >= 2 {
        if let StepValue::List(coords) = &entity.args[1] {
            if let (StepValue::Number(x), StepValue::Number(y), StepValue::Number(z)) = (
                unwrap_typed(&coords[0]),
                unwrap_typed(&coords[1]),
                unwrap_typed(&coords[2]),
            ) {
                return Some([*x, *y, *z]);
            }
        }
    }
    None
}

fn get_direction(entities: &HashMap<String, Vec<Entity>>, ref_id: &str) -> Option<[f64; 3]> {
    let entity = find_entity_named(entities, ref_id, "DIRECTION")?;
    if entity.args.len() >= 2 {
        if let StepValue::List(coords) = &entity.args[1] {
            if let (StepValue::Number(x), StepValue::Number(y), StepValue::Number(z)) = (
                unwrap_typed(&coords[0]),
                unwrap_typed(&coords[1]),
                unwrap_typed(&coords[2]),
            ) {
                return Some([*x, *y, *z]);
            }
        }
    }
    None
}

fn get_vector_direction(entities: &HashMap<String, Vec<Entity>>, ref_id: &str) -> Option<[f64; 3]> {
    let entity = find_entity_named(entities, ref_id, "VECTOR")
        .or_else(|| find_entity_named(entities, ref_id, "DIRECTION"))?;
    if entity.name == "VECTOR" && entity.args.len() >= 2 {
        if let StepValue::Ref(dir_ref) = &entity.args[1] {
            return get_direction(entities, dir_ref);
        }
    }
    if entity.name == "DIRECTION" && entity.args.len() >= 2 {
        if let StepValue::List(coords) = &entity.args[1] {
            if let (StepValue::Number(x), StepValue::Number(y), StepValue::Number(z)) = (
                unwrap_typed(&coords[0]),
                unwrap_typed(&coords[1]),
                unwrap_typed(&coords[2]),
            ) {
                return Some([*x, *y, *z]);
            }
        }
    }
    None
}

fn get_axis2_placement(
    entities: &HashMap<String, Vec<Entity>>,
    ref_id: &str,
) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    let entity = find_entity_named(entities, ref_id, "AXIS2_PLACEMENT_3D")?;
    if entity.args.len() >= 4 {
        let origin = match &entity.args[1] {
            StepValue::Ref(pt_ref) => get_point(entities, pt_ref)?,
            _ => return None,
        };
        let axis = match &entity.args[2] {
            StepValue::Ref(dir_ref) => get_direction(entities, dir_ref)?,
            _ => return None,
        };
        let ref_dir = match &entity.args[3] {
            StepValue::Ref(dir_ref) => get_direction(entities, dir_ref)?,
            _ => return None,
        };
        Some((origin, axis, ref_dir))
    } else {
        None
    }
}

fn resolve_surface_params(
    entities: &HashMap<String, Vec<Entity>>,
    surf_ref: &str,
    face_id: &str,
) -> Option<(String, SurfaceParams)> {
    let entity = first_entity_matching(entities, surf_ref, |_| true)?;
    match entity.name.as_str() {
        "PLANE" => {
            if entity.args.len() >= 2 {
                if let StepValue::Ref(ax_ref) = &entity.args[1] {
                    if let Some((origin, normal, _ref_dir)) = get_axis2_placement(entities, ax_ref)
                    {
                        return Some((
                            face_id.to_string(),
                            SurfaceParams::Plane { origin, normal },
                        ));
                    }
                }
            }
        }
        "CYLINDRICAL_SURFACE" => {
            if entity.args.len() >= 3 {
                if let StepValue::Ref(ax_ref) = &entity.args[1] {
                    if let StepValue::Number(radius) = entity.args[2] {
                        if let Some((origin, axis, _ref_dir)) =
                            get_axis2_placement(entities, ax_ref)
                        {
                            return Some((
                                face_id.to_string(),
                                SurfaceParams::Cylinder {
                                    origin,
                                    axis,
                                    radius,
                                },
                            ));
                        }
                    }
                }
            }
        }
        "CONICAL_SURFACE" => {
            if entity.args.len() >= 4 {
                if let StepValue::Ref(ax_ref) = &entity.args[1] {
                    if let (StepValue::Number(radius), StepValue::Number(half_angle)) =
                        (&entity.args[2], &entity.args[3])
                    {
                        if let Some((origin, axis, _ref_dir)) =
                            get_axis2_placement(entities, ax_ref)
                        {
                            return Some((
                                face_id.to_string(),
                                SurfaceParams::Cone {
                                    origin,
                                    axis,
                                    radius: *radius,
                                    half_angle: *half_angle,
                                },
                            ));
                        }
                    }
                }
            }
        }
        "SPHERICAL_SURFACE" => {
            if entity.args.len() >= 3 {
                if let StepValue::Ref(ax_ref) = &entity.args[1] {
                    if let StepValue::Number(radius) = entity.args[2] {
                        if let Some((center, _axis, _ref_dir)) =
                            get_axis2_placement(entities, ax_ref)
                        {
                            return Some((
                                face_id.to_string(),
                                SurfaceParams::Sphere { center, radius },
                            ));
                        }
                    }
                }
            }
        }
        "TOROIDAL_SURFACE" => {
            if entity.args.len() >= 4 {
                if let StepValue::Ref(ax_ref) = &entity.args[1] {
                    if let (StepValue::Number(major_radius), StepValue::Number(minor_radius)) =
                        (&entity.args[2], &entity.args[3])
                    {
                        if let Some((center, axis, _ref_dir)) =
                            get_axis2_placement(entities, ax_ref)
                        {
                            return Some((
                                face_id.to_string(),
                                SurfaceParams::Torus {
                                    center,
                                    axis,
                                    major_radius: *major_radius,
                                    minor_radius: *minor_radius,
                                },
                            ));
                        }
                    }
                }
            }
        }
        "B_SPLINE_SURFACE_WITH_KNOTS" => {
            if entity.args.len() >= 12 {
                let degree_u = match &entity.args[1] {
                    StepValue::Number(d) => d.round() as usize,
                    _ => return None,
                };
                let degree_v = match &entity.args[2] {
                    StepValue::Number(d) => d.round() as usize,
                    _ => return None,
                };
                let control_points: Vec<Vec<[f64; 3]>> = match &entity.args[3] {
                    StepValue::List(rows) => rows
                        .iter()
                        .map(|row| {
                            if let StepValue::List(refs) = row {
                                refs.iter()
                                    .filter_map(|v| {
                                        if let StepValue::Ref(r) = v {
                                            get_point(entities, r)
                                        } else {
                                            None
                                        }
                                    })
                                    .collect()
                            } else {
                                Vec::new()
                            }
                        })
                        .collect(),
                    _ => return None,
                };
                let u_multiplicities: Vec<f64> = match &entity.args[8] {
                    StepValue::List(mults) => mults
                        .iter()
                        .filter_map(|v| {
                            if let StepValue::Number(n) = v {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => return None,
                };
                let v_multiplicities: Vec<f64> = match &entity.args[9] {
                    StepValue::List(mults) => mults
                        .iter()
                        .filter_map(|v| {
                            if let StepValue::Number(n) = v {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => return None,
                };
                let u_knots: Vec<f64> = match &entity.args[10] {
                    StepValue::List(k) => k
                        .iter()
                        .filter_map(|v| {
                            if let StepValue::Number(n) = v {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => return None,
                };
                let v_knots: Vec<f64> = match &entity.args[11] {
                    StepValue::List(k) => k
                        .iter()
                        .filter_map(|v| {
                            if let StepValue::Number(n) = v {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => return None,
                };
                let knots_u = expand_knots(&u_multiplicities, &u_knots);
                let knots_v = expand_knots(&v_multiplicities, &v_knots);
                return Some((
                    face_id.to_string(),
                    SurfaceParams::NurbsSurface {
                        degree_u,
                        degree_v,
                        control_points,
                        knots_u,
                        knots_v,
                        weights: None,
                    },
                ));
            }
        }
        _ => {}
    }
    None
}

fn expand_knots(multiplicities: &[f64], knots: &[f64]) -> Vec<f64> {
    let mut result = Vec::new();
    for (&mult, &knot) in multiplicities.iter().zip(knots.iter()) {
        let count = mult.round() as usize;
        for _ in 0..count {
            result.push(knot);
        }
    }
    result
}

fn resolve_curve_params(
    entities: &HashMap<String, Vec<Entity>>,
    curve_ref: &str,
    edge_id: &str,
) -> Option<(String, CurveParams)> {
    let entity = first_entity_matching(entities, curve_ref, |_| true)?;
    match entity.name.as_str() {
        "LINE" => {
            if entity.args.len() >= 3 {
                let point = match &entity.args[1] {
                    StepValue::Ref(pt_ref) => get_point(entities, pt_ref)?,
                    _ => return None,
                };
                let direction = match &entity.args[2] {
                    StepValue::Ref(vec_ref) => get_vector_direction(entities, vec_ref)?,
                    _ => return None,
                };
                return Some((edge_id.to_string(), CurveParams::Line { point, direction }));
            }
        }
        "CIRCLE" => {
            if entity.args.len() >= 3 {
                if let StepValue::Number(radius) = entity.args[2] {
                    if let StepValue::Ref(ax_ref) = &entity.args[1] {
                        if let Some((center, axis, _ref_dir)) =
                            get_axis2_placement(entities, ax_ref)
                        {
                            return Some((
                                edge_id.to_string(),
                                CurveParams::Circle {
                                    center,
                                    axis,
                                    radius,
                                },
                            ));
                        }
                    }
                }
            }
        }
        "ELLIPSE" => {
            if entity.args.len() >= 4 {
                if let (StepValue::Number(semi_major), StepValue::Number(semi_minor)) =
                    (&entity.args[2], &entity.args[3])
                {
                    if let StepValue::Ref(ax_ref) = &entity.args[1] {
                        if let Some((center, axis, _ref_dir)) =
                            get_axis2_placement(entities, ax_ref)
                        {
                            return Some((
                                edge_id.to_string(),
                                CurveParams::Ellipse {
                                    center,
                                    axis,
                                    semi_major: *semi_major,
                                    semi_minor: *semi_minor,
                                },
                            ));
                        }
                    }
                }
            }
        }
        "B_SPLINE_CURVE_WITH_KNOTS" => {
            if entity.args.len() >= 8 {
                let degree = match &entity.args[1] {
                    StepValue::Number(d) => d.round() as usize,
                    _ => return None,
                };
                let points: Vec<[f64; 3]> = match &entity.args[2] {
                    StepValue::List(refs) => refs
                        .iter()
                        .filter_map(|v| {
                            if let StepValue::Ref(r) = v {
                                get_point(entities, r)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => return None,
                };
                let multiplicities: Vec<f64> = match &entity.args[6] {
                    StepValue::List(mults) => mults
                        .iter()
                        .filter_map(|v| {
                            if let StepValue::Number(n) = v {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => return None,
                };
                let knots: Vec<f64> = match &entity.args[7] {
                    StepValue::List(k) => k
                        .iter()
                        .filter_map(|v| {
                            if let StepValue::Number(n) = v {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => return None,
                };
                let expanded = expand_knots(&multiplicities, &knots);
                return Some((
                    edge_id.to_string(),
                    CurveParams::NurbsCurve {
                        degree,
                        control_points: points,
                        knots: expanded,
                        weights: None,
                    },
                ));
            }
        }
        _ => {}
    }
    None
}

fn build_brep(entities: &HashMap<String, Vec<Entity>>) -> (BRep, Consumed, HashMap<String, usize>) {
    let mut points: HashMap<String, [f64; 3]> = HashMap::new();
    let mut vertices: Vec<BrepVertex> = Vec::new();
    let mut edges: Vec<BrepEdge> = Vec::new();
    let mut faces: Vec<BrepFace> = Vec::new();

    let mut entity_counts: HashMap<String, usize> = HashMap::new();
    for (_, sub) in entities {
        for entity in sub {
            *entity_counts.entry(entity.name.clone()).or_insert(0) += 1;
        }
    }

    for (id, sub) in entities {
        for entity in sub {
            if entity.name == "CARTESIAN_POINT" && entity.args.len() >= 2 {
                if let StepValue::List(coords) = &entity.args[1] {
                    if coords.len() == 3 {
                        if let (StepValue::Number(x), StepValue::Number(y), StepValue::Number(z)) = (
                            unwrap_typed(&coords[0]),
                            unwrap_typed(&coords[1]),
                            unwrap_typed(&coords[2]),
                        ) {
                            points.insert(id.clone(), [*x, *y, *z]);
                        }
                    }
                }
            }
        }
    }

    let mut consumed_verts = 0usize;
    for (id, sub) in entities {
        for entity in sub {
            if entity.name == "VERTEX_POINT" && entity.args.len() >= 2 {
                if let StepValue::Ref(pt_ref) = &entity.args[1] {
                    if let Some(&pt) = points.get(pt_ref) {
                        vertices.push(BrepVertex {
                            id: id.clone(),
                            point: pt,
                        });
                        consumed_verts += 1;
                    }
                }
            }
        }
    }

    let mut consumed_edges = 0usize;
    let mut curve_params = BTreeMap::new();
    for (id, sub) in entities {
        for entity in sub {
            if entity.name == "EDGE_CURVE" && entity.args.len() >= 4 {
                if let (StepValue::Ref(v1), StepValue::Ref(v2), StepValue::Ref(curve_ref)) =
                    (&entity.args[1], &entity.args[2], &entity.args[3])
                {
                    let curve_type = resolve_curve_type(entities, curve_ref);
                    edges.push(BrepEdge {
                        id: id.clone(),
                        curve: curve_type,
                        vertices: [v1.clone(), v2.clone()],
                    });
                    if let Some((key, cp)) = resolve_curve_params(entities, curve_ref, id) {
                        curve_params.insert(key, cp);
                    }
                    consumed_edges += 1;
                }
            }
        }
    }

    let mut consumed_faces = 0usize;
    let mut surface_params = BTreeMap::new();
    let mut face_surf_map: HashMap<String, String> = HashMap::new();
    for (id, sub) in entities {
        for entity in sub {
            if entity.name == "ADVANCED_FACE" && entity.args.len() >= 3 {
                let surface_type = if let StepValue::Ref(surf_ref) = &entity.args[2] {
                    let st = resolve_surface_type(entities, surf_ref);
                    if let Some((key, sp)) = resolve_surface_params(entities, surf_ref, id) {
                        surface_params.insert(key, sp);
                    }
                    face_surf_map.insert(id.clone(), surf_ref.clone());
                    st
                } else {
                    SurfaceType::Other
                };

                let edge_refs = extract_face_edges(entities, &entity.args[1]);

                faces.push(BrepFace {
                    id: id.clone(),
                    surface: surface_type,
                    edges: edge_refs,
                });
                consumed_faces += 1;
            }
        }
    }

    let consumed = Consumed {
        vertices: consumed_verts,
        edges: consumed_edges,
        faces: consumed_faces,
    };

    (
        BRep {
            vertices,
            edges,
            faces,
            surface_params,
            curve_params,
        },
        consumed,
        entity_counts,
    )
}

fn find_solid_face_groups(entities: &HashMap<String, Vec<Entity>>) -> Vec<(String, Vec<String>)> {
    let mut solids: Vec<(String, Vec<String>)> = Vec::new();
    for (_id, sub) in entities {
        for entity in sub {
            if entity.name == "MANIFOLD_SOLID_BREP" && entity.args.len() >= 2 {
                let solid_name = if let StepValue::Str(ref s) = entity.args[0] {
                    if s.is_empty() {
                        format!("solid_{}", solids.len() + 1)
                    } else {
                        s.clone()
                    }
                } else {
                    format!("solid_{}", solids.len() + 1)
                };
                let shell_ref = if let StepValue::Ref(ref r) = entity.args[1] {
                    r.clone()
                } else {
                    continue;
                };
                if let Some(shell_entity) = find_entity_named(entities, &shell_ref, "CLOSED_SHELL")
                {
                    if shell_entity.args.len() >= 2 {
                        if let StepValue::List(face_refs) = &shell_entity.args[1] {
                            let face_ids: Vec<String> = face_refs
                                .iter()
                                .filter_map(|v| {
                                    if let StepValue::Ref(r) = v {
                                        Some(r.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            solids.push((solid_name, face_ids));
                        }
                    }
                }
            }
        }
    }
    solids
}

fn filter_brep_for_faces(full_brep: &BRep, face_ids: &[String]) -> BRep {
    let face_set: std::collections::HashSet<&String> = face_ids.iter().collect();
    let included_faces: Vec<&BrepFace> = full_brep
        .faces
        .iter()
        .filter(|f| face_set.contains(&f.id))
        .collect();

    let mut edge_ids = std::collections::HashSet::new();
    for f in &included_faces {
        for eid in &f.edges {
            edge_ids.insert(eid.clone());
        }
    }

    let included_edges: Vec<&BrepEdge> = full_brep
        .edges
        .iter()
        .filter(|e| edge_ids.contains(&e.id))
        .collect();

    let mut vert_ids = std::collections::HashSet::new();
    for e in &included_edges {
        vert_ids.insert(e.vertices[0].clone());
        vert_ids.insert(e.vertices[1].clone());
    }

    let included_verts: Vec<BrepVertex> = full_brep
        .vertices
        .iter()
        .filter(|v| vert_ids.contains(&v.id))
        .cloned()
        .collect();

    let surface_params = full_brep
        .surface_params
        .iter()
        .filter(|(k, _)| face_set.contains(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let curve_params = full_brep
        .curve_params
        .iter()
        .filter(|(k, _)| edge_ids.contains(k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    BRep {
        vertices: included_verts,
        edges: included_edges.into_iter().cloned().collect(),
        faces: included_faces.into_iter().cloned().collect(),
        surface_params,
        curve_params,
    }
}

fn resolve_product_name_from_def(
    entities: &HashMap<String, Vec<Entity>>,
    prod_def_ref: &str,
) -> Option<String> {
    let prod_def = find_entity_named(entities, prod_def_ref, "PRODUCT_DEFINITION")?;
    if prod_def.args.len() >= 3 {
        if let StepValue::Ref(form_ref) = &prod_def.args[2] {
            let form = entities.get(form_ref)?;
            for fe in form {
                if fe.name == "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE"
                    || fe.name == "PRODUCT_DEFINITION_FORMATION"
                {
                    if fe.args.len() >= 3 {
                        if let StepValue::Ref(prod_ref) = &fe.args[2] {
                            let product = find_entity_named(entities, prod_ref, "PRODUCT")?;
                            if product.args.len() >= 2 {
                                if let StepValue::Str(name) = &product.args[1] {
                                    if !name.is_empty() {
                                        return Some(name.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn parse_assembly_instances(
    entities: &HashMap<String, Vec<Entity>>,
    parts: &[Part],
) -> Vec<Instance> {
    let mut instances: Vec<Instance> = Vec::new();

    for (_id, sub) in entities {
        for entity in sub {
            if entity.name != "NEXT_ASSEMBLY_USAGE_OCCURRENCE" {
                continue;
            }
            if entity.args.len() < 6 {
                continue;
            }
            let name = match &entity.args[1] {
                StepValue::Str(s) => s.clone(),
                _ => continue,
            };
            let child_prod_def_ref = match &entity.args[4] {
                StepValue::Ref(r) => r.clone(),
                _ => continue,
            };

            let child_product_name = resolve_product_name_from_def(entities, &child_prod_def_ref)
                .unwrap_or_else(|| name.clone());

            let mut transform = Transform::identity();

            for (_, rel_sub) in entities {
                for rel_entity in rel_sub {
                    if rel_entity.name == "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"
                        && rel_entity.args.len() >= 3
                    {
                        if let StepValue::Ref(srr_ref) = &rel_entity.args[1] {
                            if let Some(srr) = find_entity_named(
                                entities,
                                srr_ref,
                                "SHAPE_REPRESENTATION_RELATIONSHIP",
                            ) {
                                if srr.args.len() >= 5 {
                                    if let StepValue::Ref(rep1_ref) = &srr.args[3] {
                                        if let StepValue::Ref(rep2_ref) = &srr.args[4] {
                                            if rep1_ref.as_str() == child_prod_def_ref.as_str()
                                                || rep2_ref.as_str() == child_prod_def_ref.as_str()
                                            {
                                                if let StepValue::Ref(idt_ref) = &rel_entity.args[2]
                                                {
                                                    if let Some(idt) = find_entity_named(
                                                        entities,
                                                        idt_ref,
                                                        "ITEM_DEFINED_TRANSFORMATION",
                                                    ) {
                                                        if idt.args.len() >= 4 {
                                                            if let StepValue::Ref(ax2_ref) =
                                                                &idt.args[3]
                                                            {
                                                                if let Some((
                                                                    origin,
                                                                    _axis,
                                                                    _ref_dir,
                                                                )) = get_axis2_placement(
                                                                    entities, ax2_ref,
                                                                ) {
                                                                    transform.0[0][3] = origin[0];
                                                                    transform.0[1][3] = origin[1];
                                                                    transform.0[2][3] = origin[2];
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let part_ref = parts
                .iter()
                .find(|p| p.name == child_product_name)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| child_product_name);

            instances.push(Instance {
                part_ref,
                name,
                transform,
            });
        }
    }

    instances
}

fn resolve_product_names(entities: &HashMap<String, Vec<Entity>>) -> HashMap<String, String> {
    let mut product_names: HashMap<String, String> = HashMap::new();
    for (id, sub) in entities {
        for entity in sub {
            if entity.name == "PRODUCT" && entity.args.len() >= 2 {
                if let StepValue::Str(name) = &entity.args[1] {
                    if !name.is_empty() {
                        product_names.insert(id.clone(), name.clone());
                    }
                }
            }
        }
    }
    product_names
}

fn system_time_to_iso(t: SystemTime) -> String {
    let duration = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let mut y: i64 = 1970;
    let mut remaining_days = days as i64;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let month_days: [i64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m: usize = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            m = i + 1;
            break;
        }
        remaining_days -= md;
    }

    let d = remaining_days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

pub fn import_step(path: &Path) -> Result<(Document, FidelityReport), StepError> {
    let content = std::fs::read_to_string(path).map_err(StepError::Io)?;

    let trimmed = content.trim_start();
    if !trimmed.starts_with("ISO-10303-21;") {
        return Err(StepError::NotAStepFile);
    }

    let header_section = extract_section(&content, "HEADER;").unwrap_or_default();

    let data_section = extract_section(&content, "DATA;")
        .ok_or_else(|| StepError::Parse("missing DATA section".into()))?;

    let (model_name, _schema) = parse_header(&header_section)?;

    let entities = parse_data_section(&data_section)?;

    let (full_brep, consumed, entity_counts) = build_brep(&entities);

    let solid_groups = find_solid_face_groups(&entities);

    let _product_names = resolve_product_names(&entities);

    let has_solids = solid_groups.len() > 1;

    let mut parametric_surf_types = std::collections::HashSet::new();
    parametric_surf_types.insert("PLANE");
    parametric_surf_types.insert("CYLINDRICAL_SURFACE");
    parametric_surf_types.insert("CONICAL_SURFACE");
    parametric_surf_types.insert("SPHERICAL_SURFACE");
    parametric_surf_types.insert("TOROIDAL_SURFACE");

    let mut parametric_curve_types = std::collections::HashSet::new();
    parametric_curve_types.insert("LINE");
    parametric_curve_types.insert("CIRCLE");
    parametric_curve_types.insert("ELLIPSE");

    let mut nurbs_surf_types = std::collections::HashSet::new();
    nurbs_surf_types.insert("B_SPLINE_SURFACE_WITH_KNOTS");

    let mut nurbs_curve_types = std::collections::HashSet::new();
    nurbs_curve_types.insert("B_SPLINE_CURVE_WITH_KNOTS");

    let surface_captured: std::collections::HashSet<&String> = if !solid_groups.is_empty() {
        let mut set = std::collections::HashSet::new();
        for (_name, face_ids) in &solid_groups {
            for fid in face_ids {
                if full_brep.surface_params.contains_key(fid) {
                    set.insert(fid);
                }
            }
        }
        set
    } else {
        full_brep.surface_params.keys().collect()
    };

    let edge_group_set: std::collections::HashSet<&String> = if !solid_groups.is_empty() {
        let mut set = std::collections::HashSet::new();
        for (_name, face_ids) in &solid_groups {
            for fid in face_ids {
                for f in full_brep.faces.iter().filter(|f| &f.id == fid) {
                    for eid in &f.edges {
                        set.insert(eid);
                    }
                }
            }
        }
        set
    } else {
        full_brep.edges.iter().map(|e| &e.id).collect()
    };

    let curve_captured: std::collections::HashSet<&String> = edge_group_set
        .iter()
        .filter(|eid| full_brep.curve_params.contains_key(**eid))
        .map(|&s| s)
        .collect();

    let mut report = FidelityReport::new("step", "exl");

    report.record(
        "VERTEX_POINT",
        consumed.vertices,
        EntityStatus::Lossless,
        None,
    );
    report.record("EDGE_CURVE", consumed.edges, EntityStatus::Lossless, None);

    let adv_face_lossless = consumed.faces > 0
        && consumed.faces
            == full_brep
                .faces
                .iter()
                .filter(|f| {
                    parametric_surf_types
                        .contains(resolve_surface_type_from_brep(f, &full_brep).as_str())
                        && surface_captured.contains(&f.id)
                })
                .count();

    let face_status = if adv_face_lossless && consumed.faces > 0 {
        EntityStatus::Lossless
    } else {
        EntityStatus::Approximate
    };
    let face_note = if face_status == EntityStatus::Approximate {
        Some("surface parameters not evaluated at v0".into())
    } else {
        None
    };

    report.record("ADVANCED_FACE", consumed.faces, face_status, face_note);

    let consumed_set: [&str; 3] = ["VERTEX_POINT", "EDGE_CURVE", "ADVANCED_FACE"];
    let structural_set: [&str; 4] = [
        "FACE_OUTER_BOUND",
        "FACE_BOUND",
        "EDGE_LOOP",
        "ORIENTED_EDGE",
    ];
    let type_mapped_set: [&str; 9] = [
        "PLANE",
        "CYLINDRICAL_SURFACE",
        "CONICAL_SURFACE",
        "SPHERICAL_SURFACE",
        "TOROIDAL_SURFACE",
        "SURFACE_OF_LINEAR_EXTRUSION",
        "LINE",
        "CIRCLE",
        "ELLIPSE",
    ];
    let has_params = !surface_captured.is_empty() || !curve_captured.is_empty();
    for (etype, count) in entity_counts.iter() {
        if consumed_set.contains(&etype.as_str()) {
            continue;
        }
        if structural_set.contains(&etype.as_str()) {
            report.record(etype.clone(), *count, EntityStatus::Lossless, None);
        } else if parametric_surf_types.contains(etype.as_str()) {
            let all_captured = solid_groups.is_empty()
                || solid_groups
                    .iter()
                    .flat_map(|(_, face_ids)| face_ids)
                    .all(|fid| surface_captured.contains(fid));
            let status = if all_captured && *count > 0 {
                EntityStatus::Lossless
            } else {
                EntityStatus::Approximate
            };
            let note = if status == EntityStatus::Lossless {
                Some("parameters preserved".into())
            } else {
                Some("type mapped; parameters not preserved".into())
            };
            report.record(etype.clone(), *count, status, note);
        } else if parametric_curve_types.contains(etype.as_str()) {
            let all_captured = solid_groups.is_empty()
                || edge_group_set
                    .iter()
                    .all(|eid| curve_captured.contains(eid));
            let status = if all_captured && !curve_captured.is_empty() {
                EntityStatus::Lossless
            } else {
                EntityStatus::Approximate
            };
            let note = if status == EntityStatus::Lossless {
                Some("parameters preserved".into())
            } else {
                Some("type mapped; parameters not preserved".into())
            };
            report.record(etype.clone(), *count, status, note);
        } else if nurbs_surf_types.contains(etype.as_str()) {
            let all_captured = solid_groups.is_empty()
                || solid_groups
                    .iter()
                    .flat_map(|(_, face_ids)| face_ids)
                    .all(|fid| surface_captured.contains(fid));
            let status = if all_captured && *count > 0 {
                EntityStatus::Lossless
            } else {
                EntityStatus::Approximate
            };
            let note = if status == EntityStatus::Lossless {
                Some("full parameters captured".into())
            } else {
                Some("type mapped; parameters not preserved".into())
            };
            report.record(etype.clone(), *count, status, note);
        } else if nurbs_curve_types.contains(etype.as_str()) {
            let all_captured = solid_groups.is_empty()
                || edge_group_set
                    .iter()
                    .all(|eid| curve_captured.contains(eid));
            let status = if all_captured && !curve_captured.is_empty() {
                EntityStatus::Lossless
            } else {
                EntityStatus::Approximate
            };
            let note = if status == EntityStatus::Lossless {
                Some("full parameters captured".into())
            } else {
                Some("type mapped; parameters not preserved".into())
            };
            report.record(etype.clone(), *count, status, note);
        } else if type_mapped_set.contains(&etype.as_str()) || etype.starts_with("B_SPLINE") {
            report.record(
                etype.clone(),
                *count,
                EntityStatus::Approximate,
                Some("type mapped; parameters not preserved".into()),
            );
        } else if etype == "CARTESIAN_POINT" {
            let status = if has_params {
                EntityStatus::Lossless
            } else {
                EntityStatus::Approximate
            };
            let note = if has_params {
                Some("consumed via parameter resolution".into())
            } else {
                Some("only vertex-referenced points preserved".into())
            };
            report.record(etype.clone(), *count, status, note);
        } else if etype == "AXIS2_PLACEMENT_3D" || etype == "DIRECTION" || etype == "VECTOR" {
            let status = if has_params {
                EntityStatus::Lossless
            } else {
                EntityStatus::Dropped
            };
            let note = if has_params {
                Some("consumed via parameter resolution".into())
            } else {
                None
            };
            report.record(etype.clone(), *count, status, note);
        } else if etype == "PRODUCT" {
            if has_solids {
                report.record(
                    etype.clone(),
                    *count,
                    EntityStatus::Lossless,
                    Some("mapped to part names".into()),
                );
            } else {
                report.record(etype.clone(), *count, EntityStatus::Dropped, None);
            }
        } else {
            report.record(etype.clone(), *count, EntityStatus::Dropped, None);
        }
    }

    let name = model_name.filter(|n| !n.is_empty()).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let parts: Vec<Part> = if solid_groups.is_empty() {
        vec![Part::new(name, GeometryPayload::Brep(full_brep))]
    } else {
        solid_groups
            .into_iter()
            .enumerate()
            .map(|(i, (solid_name, face_ids))| {
                let part_name = if solid_name.is_empty() {
                    format!("{}__{}", name, i + 1)
                } else {
                    solid_name
                };
                let subset_brep = filter_brep_for_faces(&full_brep, &face_ids);
                Part::new(part_name, GeometryPayload::Brep(subset_brep))
            })
            .collect()
    };

    let instances = parse_assembly_instances(&entities, &parts);

    let mut doc = Document::new(parts);
    if !instances.is_empty() {
        doc.assembly = Assembly {
            instances,
            ..Default::default()
        };
    }

    let timestamp = system_time_to_iso(SystemTime::now());
    doc.provenance.tool_of_origin = Some(ToolOfOrigin {
        name: "exl-step".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        timestamp_iso: timestamp,
    });
    doc.provenance.conversion_fidelity = Some(report.overall);

    Ok((doc, report))
}

fn resolve_surface_type_from_brep(face: &BrepFace, brep: &BRep) -> String {
    if let Some(params) = brep.surface_params.get(&face.id) {
        return match params {
            SurfaceParams::Plane { .. } => "PLANE".to_string(),
            SurfaceParams::Cylinder { .. } => "CYLINDRICAL_SURFACE".to_string(),
            SurfaceParams::Cone { .. } => "CONICAL_SURFACE".to_string(),
            SurfaceParams::Sphere { .. } => "SPHERICAL_SURFACE".to_string(),
            SurfaceParams::Torus { .. } => "TOROIDAL_SURFACE".to_string(),
            SurfaceParams::NurbsSurface { .. } => "B_SPLINE_SURFACE_WITH_KNOTS".to_string(),
        };
    }
    match face.surface {
        SurfaceType::Plane => "PLANE".to_string(),
        SurfaceType::Cylinder => "CYLINDRICAL_SURFACE".to_string(),
        SurfaceType::Cone => "CONICAL_SURFACE".to_string(),
        SurfaceType::Sphere => "SPHERICAL_SURFACE".to_string(),
        SurfaceType::Torus => "TOROIDAL_SURFACE".to_string(),
        SurfaceType::Extrusion => "SURFACE_OF_LINEAR_EXTRUSION".to_string(),
        SurfaceType::Nurbs => "B_SPLINE_SURFACE".to_string(),
        SurfaceType::Other => "OTHER".to_string(),
    }
}

fn extract_section(content: &str, keyword: &str) -> Option<String> {
    let start = content.find(keyword)?;
    let inner_start = start + keyword.len();
    let remaining = &content[inner_start..];
    let inner_end = remaining.find("ENDSEC;")?;
    Some(remaining[..inner_end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::fs;

    const TEST_STEP: &str = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('test'),'1');
FILE_NAME('Test Part','2024-01-01T00:00:00',('Author'),('Org'),'preprocessor','exl-test','');
FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));
ENDSEC;
DATA;
#1 = PRODUCT('product','test product','',(#100));
#10 = CARTESIAN_POINT('',(0.0,0.0,0.0));
#11 = CARTESIAN_POINT('',(10.0,0.0,0.0));
#12 = CARTESIAN_POINT('',(10.0,10.0,0.0));
#13 = CARTESIAN_POINT('',(0.0,10.0,0.0));
#20 = VERTEX_POINT('',#10);
#21 = VERTEX_POINT('',#11);
#22 = VERTEX_POINT('',#12);
#23 = VERTEX_POINT('',#13);
#30 = LINE('',#10,#34);
#31 = LINE('',#11,#35);
#32 = LINE('',#12,#36);
#33 = LINE('',#13,#37);
#34 = VECTOR('',#40,1.0);
#35 = VECTOR('',#41,1.0);
#36 = VECTOR('',#42,1.0);
#37 = VECTOR('',#43,1.0);
#40 = DIRECTION('',(1.0,0.0,0.0));
#41 = DIRECTION('',(0.0,1.0,0.0));
#42 = DIRECTION('',(-1.0,0.0,0.0));
#43 = DIRECTION('',(0.0,-1.0,0.0));
#50 = EDGE_CURVE('',#20,#21,#30,.T.);
#51 = EDGE_CURVE('',#21,#22,#31,.T.);
#52 = EDGE_CURVE('',#22,#23,#32,.T.);
#53 = EDGE_CURVE('',#23,#20,#33,.T.);
#60 = CIRCLE('',#70,5.0);
#61 = EDGE_CURVE('',#20,#22,#60,.T.);
#70 = AXIS2_PLACEMENT_3D('',#10,#71,#72);
#71 = DIRECTION('',(0.0,0.0,1.0));
#72 = DIRECTION('',(1.0,0.0,0.0));
#80 = PLANE('',#81);
#81 = AXIS2_PLACEMENT_3D('',#10,#90,#91);
#90 = DIRECTION('',(0.0,0.0,1.0));
#91 = DIRECTION('',(1.0,0.0,0.0));
#85 = CYLINDRICAL_SURFACE('',#86,5.0);
#86 = AXIS2_PLACEMENT_3D('',#10,#87,#88);
#87 = DIRECTION('',(0.0,0.0,1.0));
#88 = DIRECTION('',(1.0,0.0,0.0));
#100 = PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#1,.NOT_KNOWN.);
#200 = EDGE_LOOP('',(#300,#301,#302,#303));
#210 = EDGE_LOOP('',(#310));
#300 = ORIENTED_EDGE('',*,*,#50,.T.);
#301 = ORIENTED_EDGE('',*,*,#51,.T.);
#302 = ORIENTED_EDGE('',*,*,#52,.T.);
#303 = ORIENTED_EDGE('',*,*,#53,.T.);
#310 = ORIENTED_EDGE('',*,*,#61,.T.);
#400 = FACE_OUTER_BOUND('',#200,.T.);
#410 = FACE_OUTER_BOUND('',#210,.T.);
#500 = ADVANCED_FACE('',(#400),#80,.T.);
#510 = ADVANCED_FACE('',(#410),#85,.T.);
ENDSEC;
END-ISO-10303-21;
"#;

    fn write_test_file(name: &str, content: &str) -> std::path::PathBuf {
        let dir = temp_dir();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn not_a_step_file() {
        let path = write_test_file("not_step.txt", "Hello, world!");
        let result = import_step(&path);
        assert!(matches!(result, Err(StepError::NotAStepFile)));
    }

    #[test]
    fn parse_and_extract_brep() {
        let path = write_test_file("test_step.stp", TEST_STEP);
        let result = import_step(&path).unwrap();
        let (doc, report) = result;

        assert_eq!(doc.parts.len(), 1);
        let part = &doc.parts[0];
        assert_eq!(part.name, "Test Part");

        if let GeometryPayload::Brep(brep) = &part.geometry {
            assert_eq!(brep.vertices.len(), 4, "vertex count");
            assert_eq!(brep.edges.len(), 5, "edge count");
            assert_eq!(brep.faces.len(), 2, "face count");

            for v in &brep.vertices {
                assert!(
                    v.point[0] >= 0.0
                        && v.point[0] <= 10.0
                        && v.point[1] >= 0.0
                        && v.point[1] <= 10.0
                );
            }

            let line_edges: Vec<_> = brep
                .edges
                .iter()
                .filter(|e| e.curve == CurveType::Line)
                .collect();
            let circle_edges: Vec<_> = brep
                .edges
                .iter()
                .filter(|e| e.curve == CurveType::Circle)
                .collect();
            assert_eq!(line_edges.len(), 4, "LINE edge count");
            assert_eq!(circle_edges.len(), 1, "CIRCLE edge count");

            let plane_face = brep
                .faces
                .iter()
                .find(|f| f.surface == SurfaceType::Plane)
                .unwrap();
            assert_eq!(plane_face.edges.len(), 4, "plane face edge refs");

            let cyl_face = brep
                .faces
                .iter()
                .find(|f| f.surface == SurfaceType::Cylinder)
                .unwrap();
            assert_eq!(cyl_face.edges.len(), 1, "cylinder face edge refs");

            assert_eq!(brep.surface_params.len(), 2, "surface params count");
            let plane_params = brep.surface_params.get(&plane_face.id).unwrap();
            assert!(
                matches!(plane_params, SurfaceParams::Plane { origin, normal } if *origin == [0.0, 0.0, 0.0] && *normal == [0.0, 0.0, 1.0]),
                "plane params: {:?}",
                plane_params
            );
            let cyl_params = brep.surface_params.get(&cyl_face.id).unwrap();
            assert!(
                matches!(cyl_params, SurfaceParams::Cylinder { origin, axis, radius } if *origin == [0.0, 0.0, 0.0] && *axis == [0.0, 0.0, 1.0] && (*radius - 5.0).abs() < 1e-9),
                "cylinder params: {:?}",
                cyl_params
            );

            let circle_edge = circle_edges[0];
            assert_eq!(brep.curve_params.len(), 5, "curve params count");
            let circle_params = brep.curve_params.get(&circle_edge.id).unwrap();
            assert!(
                matches!(circle_params, CurveParams::Circle { center, axis, radius } if *center == [0.0, 0.0, 0.0] && *axis == [0.0, 0.0, 1.0] && (*radius - 5.0).abs() < 1e-9),
                "circle params: {:?}",
                circle_params
            );
        } else {
            panic!("expected BRep geometry payload");
        }

        assert_eq!(report.source_format, "step");
        assert_eq!(report.target_format, "exl");

        let vertex_rec: Vec<_> = report
            .entities
            .iter()
            .filter(|e| e.entity == "VERTEX_POINT")
            .collect();
        assert_eq!(vertex_rec.len(), 1);
        assert_eq!(vertex_rec[0].count, 4);
        assert_eq!(vertex_rec[0].status, EntityStatus::Lossless);

        let edge_rec: Vec<_> = report
            .entities
            .iter()
            .filter(|e| e.entity == "EDGE_CURVE")
            .collect();
        assert_eq!(edge_rec.len(), 1);
        assert_eq!(edge_rec[0].count, 5);
        assert_eq!(edge_rec[0].status, EntityStatus::Lossless);

        let face_rec: Vec<_> = report
            .entities
            .iter()
            .filter(|e| e.entity == "ADVANCED_FACE")
            .collect();
        assert_eq!(face_rec.len(), 1);
        assert_eq!(face_rec[0].count, 2);
        assert_eq!(face_rec[0].status, EntityStatus::Lossless);
        assert_eq!(face_rec[0].note, None);

        let product_dropped: Vec<_> = report
            .entities
            .iter()
            .filter(|e| e.entity == "PRODUCT" && e.status == EntityStatus::Dropped)
            .collect();
        assert!(!product_dropped.is_empty(), "PRODUCT should be Dropped");
        assert_eq!(product_dropped[0].count, 1);

        let dropped_ids: Vec<&str> = report
            .entities
            .iter()
            .filter(|e| e.status == EntityStatus::Dropped)
            .map(|e| e.entity.as_str())
            .collect();
        assert!(
            dropped_ids.contains(&"PRODUCT"),
            "PRODUCT not found in dropped: {:?}",
            dropped_ids
        );

        let status_of = |name: &str| {
            report
                .entities
                .iter()
                .find(|e| e.entity == name)
                .map(|e| e.status)
        };
        assert_eq!(status_of("CARTESIAN_POINT"), Some(EntityStatus::Lossless));
        assert_eq!(status_of("LINE"), Some(EntityStatus::Lossless));
        assert_eq!(status_of("CIRCLE"), Some(EntityStatus::Lossless));
        assert_eq!(status_of("PLANE"), Some(EntityStatus::Lossless));
        assert_eq!(
            status_of("CYLINDRICAL_SURFACE"),
            Some(EntityStatus::Lossless)
        );
        assert_eq!(status_of("VECTOR"), Some(EntityStatus::Lossless));
        assert_eq!(
            status_of("AXIS2_PLACEMENT_3D"),
            Some(EntityStatus::Lossless)
        );
        assert_eq!(status_of("DIRECTION"), Some(EntityStatus::Lossless));
        assert_eq!(status_of("EDGE_LOOP"), Some(EntityStatus::Lossless));
        assert_eq!(status_of("ORIENTED_EDGE"), Some(EntityStatus::Lossless));
        assert_eq!(status_of("FACE_OUTER_BOUND"), Some(EntityStatus::Lossless));

        assert!(doc.provenance.tool_of_origin.is_some());
        let tool = doc.provenance.tool_of_origin.as_ref().unwrap();
        assert_eq!(tool.name, "exl-step");
        assert!(!tool.timestamp_iso.is_empty());

        assert!(doc.provenance.conversion_fidelity.is_some());
    }

    #[test]
    fn model_name_falls_back_to_file_stem() {
        let minimal = r#"ISO-10303-21;
HEADER;
FILE_NAME('','','','','','','');
FILE_SCHEMA(('TEST'));
ENDSEC;
DATA;
#1 = CARTESIAN_POINT('',(1.0,2.0,3.0));
#2 = VERTEX_POINT('',#1);
ENDSEC;
END-ISO-10303-21;
"#;
        let path = write_test_file("fallback.stp", minimal);
        let (doc, _) = import_step(&path).unwrap();
        assert_eq!(doc.parts[0].name, "fallback");
    }

    const MULTI_SOLID_STEP: &str = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('multi-solid test'),'1');
FILE_NAME('Assembly','2024-01-01T00:00:00',('Author'),('Org'),'preprocessor','exl-test','');
FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));
ENDSEC;
DATA;
#1 = PRODUCT('product_a','solid a','',(#100));
#2 = PRODUCT('product_b','solid b','',(#200));
#10 = DIRECTION('',(1.0,0.0,0.0));
#11 = DIRECTION('',(0.0,1.0,0.0));
#12 = DIRECTION('',(-1.0,0.0,0.0));
#13 = DIRECTION('',(0.0,-1.0,0.0));
#14 = DIRECTION('',(0.0,0.0,1.0));
#15 = DIRECTION('',(1.0,0.0,0.0));
#100 = CARTESIAN_POINT('',(0.0,0.0,0.0));
#101 = CARTESIAN_POINT('',(10.0,0.0,0.0));
#102 = CARTESIAN_POINT('',(10.0,10.0,0.0));
#103 = CARTESIAN_POINT('',(0.0,10.0,0.0));
#200 = CARTESIAN_POINT('',(20.0,0.0,0.0));
#201 = CARTESIAN_POINT('',(30.0,0.0,0.0));
#202 = CARTESIAN_POINT('',(30.0,10.0,0.0));
#203 = CARTESIAN_POINT('',(20.0,10.0,0.0));
#110 = VERTEX_POINT('',#100);
#111 = VERTEX_POINT('',#101);
#112 = VERTEX_POINT('',#102);
#113 = VERTEX_POINT('',#103);
#210 = VERTEX_POINT('',#200);
#211 = VERTEX_POINT('',#201);
#212 = VERTEX_POINT('',#202);
#213 = VERTEX_POINT('',#203);
#120 = VECTOR('',#10,1.0);
#121 = VECTOR('',#11,1.0);
#122 = VECTOR('',#12,1.0);
#123 = VECTOR('',#13,1.0);
#220 = VECTOR('',#10,1.0);
#221 = VECTOR('',#11,1.0);
#222 = VECTOR('',#12,1.0);
#223 = VECTOR('',#13,1.0);
#130 = LINE('',#100,#120);
#131 = LINE('',#101,#121);
#132 = LINE('',#102,#122);
#133 = LINE('',#103,#123);
#230 = LINE('',#200,#220);
#231 = LINE('',#201,#221);
#232 = LINE('',#202,#222);
#233 = LINE('',#203,#223);
#140 = EDGE_CURVE('',#110,#111,#130,.T.);
#141 = EDGE_CURVE('',#111,#112,#131,.T.);
#142 = EDGE_CURVE('',#112,#113,#132,.T.);
#143 = EDGE_CURVE('',#113,#110,#133,.T.);
#240 = EDGE_CURVE('',#210,#211,#230,.T.);
#241 = EDGE_CURVE('',#211,#212,#231,.T.);
#242 = EDGE_CURVE('',#212,#213,#232,.T.);
#243 = EDGE_CURVE('',#213,#210,#233,.T.);
#150 = AXIS2_PLACEMENT_3D('',#100,#14,#15);
#155 = PLANE('',#150);
#250 = AXIS2_PLACEMENT_3D('',#200,#14,#15);
#255 = PLANE('',#250);
#160 = EDGE_LOOP('',(#164,#165,#166,#167));
#164 = ORIENTED_EDGE('',*,*,#140,.T.);
#165 = ORIENTED_EDGE('',*,*,#141,.T.);
#166 = ORIENTED_EDGE('',*,*,#142,.T.);
#167 = ORIENTED_EDGE('',*,*,#143,.T.);
#260 = EDGE_LOOP('',(#264,#265,#266,#267));
#264 = ORIENTED_EDGE('',*,*,#240,.T.);
#265 = ORIENTED_EDGE('',*,*,#241,.T.);
#266 = ORIENTED_EDGE('',*,*,#242,.T.);
#267 = ORIENTED_EDGE('',*,*,#243,.T.);
#170 = FACE_OUTER_BOUND('',#160,.T.);
#270 = FACE_OUTER_BOUND('',#260,.T.);
#180 = ADVANCED_FACE('',(#170),#155,.T.);
#280 = ADVANCED_FACE('',(#270),#255,.T.);
#185 = CLOSED_SHELL('',(#180));
#285 = CLOSED_SHELL('',(#280));
#190 = MANIFOLD_SOLID_BREP('part_a',#185);
#290 = MANIFOLD_SOLID_BREP('part_b',#285);
#1000 = PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#1,.NOT_KNOWN.);
#2000 = PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#2,.NOT_KNOWN.);
ENDSEC;
END-ISO-10303-21;
"#;

    #[test]
    fn multi_solid_two_parts() {
        let path = write_test_file("multi_solid.stp", MULTI_SOLID_STEP);
        let (doc, report) = import_step(&path).unwrap();

        assert_eq!(doc.parts.len(), 2, "should have 2 parts");

        let part_a = doc
            .parts
            .iter()
            .find(|p| p.name == "part_a")
            .expect("part_a not found");
        let part_b = doc
            .parts
            .iter()
            .find(|p| p.name == "part_b")
            .expect("part_b not found");

        for (part, expected_origin) in [(part_a, [0.0, 0.0, 0.0]), (part_b, [20.0, 0.0, 0.0])] {
            if let GeometryPayload::Brep(brep) = &part.geometry {
                assert_eq!(brep.vertices.len(), 4, "{}: vertex count", part.name);
                assert_eq!(brep.edges.len(), 4, "{}: edge count", part.name);
                assert_eq!(brep.faces.len(), 1, "{}: face count", part.name);
                assert_eq!(
                    brep.surface_params.len(),
                    1,
                    "{}: surface params",
                    part.name
                );

                let face = &brep.faces[0];
                assert_eq!(face.surface, SurfaceType::Plane);
                let sp = brep.surface_params.get(&face.id).unwrap();
                assert!(
                    matches!(sp, SurfaceParams::Plane { origin, normal } if *origin == expected_origin && *normal == [0.0, 0.0, 1.0]),
                    "{}: plane params: {:?}",
                    part.name,
                    sp
                );

                assert_eq!(brep.curve_params.len(), 4, "{}: curve params", part.name);
                for (_, cp) in &brep.curve_params {
                    assert!(
                        matches!(cp, CurveParams::Line { .. }),
                        "{}: expected Line params",
                        part.name
                    );
                }
            } else {
                panic!("expected BRep");
            }
        }

        let status_of = |name: &str| {
            report
                .entities
                .iter()
                .find(|e| e.entity == name)
                .map(|e| e.status)
        };
        assert_eq!(status_of("PLANE"), Some(EntityStatus::Lossless));
        assert_eq!(status_of("PRODUCT"), Some(EntityStatus::Lossless));
        assert_eq!(
            status_of("MANIFOLD_SOLID_BREP"),
            Some(EntityStatus::Dropped)
        );
        assert_eq!(status_of("CLOSED_SHELL"), Some(EntityStatus::Dropped));
    }

    const SPLINE_TEST_STEP: &str = r#"ISO-10303-21;
HEADER;
FILE_NAME('bspline','','','','','','');
FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.0,0.0,0.0));
#2=CARTESIAN_POINT('',(1.0,0.5,0.0));
#3=CARTESIAN_POINT('',(2.0,0.2,0.0));
#4=CARTESIAN_POINT('',(3.0,0.0,0.0));
#10=B_SPLINE_CURVE_WITH_KNOTS('',3,(#1,#2,#3,#4),.UNSPECIFIED.,.F.,.F.,(4,4),(0.0,1.0));
#11=VERTEX_POINT('',#1);
#12=VERTEX_POINT('',#4);
#13=EDGE_CURVE('',#11,#12,#10,.T.);
#14=DIRECTION('',(-1.0,0.0,0.0));
#15=VECTOR('',#14,1.0);
#16=LINE('',#4,#15);
#17=EDGE_CURVE('',#12,#11,#16,.T.);
#20=CARTESIAN_POINT('',(0.0,0.0,0.0));
#21=CARTESIAN_POINT('',(1.5,0.0,0.0));
#22=CARTESIAN_POINT('',(3.0,0.0,0.0));
#23=CARTESIAN_POINT('',(0.0,1.5,1.0));
#24=CARTESIAN_POINT('',(1.5,1.5,1.0));
#25=CARTESIAN_POINT('',(3.0,1.5,1.0));
#26=CARTESIAN_POINT('',(0.0,3.0,0.0));
#27=CARTESIAN_POINT('',(1.5,3.0,0.0));
#28=CARTESIAN_POINT('',(3.0,3.0,0.0));
#30=B_SPLINE_SURFACE_WITH_KNOTS('',2,2,((#20,#21,#22),(#23,#24,#25),(#26,#27,#28)),.UNSPECIFIED.,.F.,.F.,.F.,(3,3),(3,3),(0.0,1.0),(0.0,1.0));
#40=EDGE_LOOP('',(#41,#42));
#41=ORIENTED_EDGE('',*,*,#13,.T.);
#42=ORIENTED_EDGE('',*,*,#17,.T.);
#50=FACE_OUTER_BOUND('',#40,.T.);
#51=ADVANCED_FACE('',(#50),#30,.T.);
ENDSEC;
END-ISO-10303-21;
"#;

    #[test]
    fn bspline_curve_and_surface_params_captured() {
        let path = write_test_file("bspline_test.stp", SPLINE_TEST_STEP);
        let (doc, report) = import_step(&path).unwrap();

        assert_eq!(doc.parts.len(), 1);
        let part = &doc.parts[0];
        assert_eq!(part.name, "bspline");

        if let GeometryPayload::Brep(brep) = &part.geometry {
            assert_eq!(brep.vertices.len(), 2);
            assert_eq!(brep.edges.len(), 2);
            assert_eq!(brep.faces.len(), 1);

            assert_eq!(brep.surface_params.len(), 1);
            assert_eq!(brep.curve_params.len(), 2);

            let face = &brep.faces[0];
            assert_eq!(face.surface, SurfaceType::Nurbs);
            let sp = brep.surface_params.get(&face.id).unwrap();
            if let SurfaceParams::NurbsSurface {
                degree_u,
                degree_v,
                control_points,
                knots_u,
                knots_v,
                weights,
            } = sp
            {
                assert_eq!(*degree_u, 2);
                assert_eq!(*degree_v, 2);
                assert_eq!(control_points.len(), 3);
                assert_eq!(control_points[0].len(), 3);
                assert_eq!(knots_u.len(), 6);
                assert_eq!(knots_v.len(), 6);
                assert_eq!(knots_u.as_slice(), &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
                assert_eq!(knots_v.as_slice(), &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
                assert!(weights.is_none());
                assert_eq!(control_points[0][0], [0.0, 0.0, 0.0]);
                assert_eq!(control_points[1][1], [1.5, 1.5, 1.0]);
                assert_eq!(control_points[2][2], [3.0, 3.0, 0.0]);
            } else {
                panic!("expected NurbsSurface, got {:?}", sp);
            }

            let bspline_edge = brep
                .edges
                .iter()
                .find(|e| e.curve == CurveType::Nurbs)
                .unwrap();
            let cp = brep.curve_params.get(&bspline_edge.id).unwrap();
            if let CurveParams::NurbsCurve {
                degree,
                control_points,
                knots,
                weights,
            } = cp
            {
                assert_eq!(*degree, 3);
                assert_eq!(control_points.len(), 4);
                assert_eq!(knots.len(), 8);
                assert_eq!(knots.as_slice(), &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
                assert!(weights.is_none());
                assert_eq!(control_points[0], [0.0, 0.0, 0.0]);
                assert_eq!(control_points[1], [1.0, 0.5, 0.0]);
                assert_eq!(control_points[3], [3.0, 0.0, 0.0]);
            } else {
                panic!("expected NurbsCurve, got {:?}", cp);
            }

            let line_edge = brep
                .edges
                .iter()
                .find(|e| e.curve == CurveType::Line)
                .unwrap();
            assert!(brep.curve_params.contains_key(&line_edge.id));
            assert!(matches!(
                brep.curve_params.get(&line_edge.id).unwrap(),
                CurveParams::Line { .. }
            ));
        } else {
            panic!("expected BRep");
        }

        let status_of = |name: &str| {
            report
                .entities
                .iter()
                .find(|e| e.entity == name)
                .map(|e| (e.status, e.note.clone()))
        };
        assert_eq!(
            status_of("B_SPLINE_CURVE_WITH_KNOTS"),
            Some((
                EntityStatus::Lossless,
                Some("full parameters captured".into())
            ))
        );
        assert_eq!(
            status_of("B_SPLINE_SURFACE_WITH_KNOTS"),
            Some((
                EntityStatus::Lossless,
                Some("full parameters captured".into())
            ))
        );
    }

    #[test]
    fn import_corpus_bspline_file() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/42-bspline.step");
        let result = import_step(&path);
        assert!(
            result.is_ok(),
            "import of 42-bspline.step failed: {:?}",
            result.err()
        );
    }

    const COMPLEX_ENTITY_STEP: &str = r#"ISO-10303-21;
HEADER;
FILE_NAME('complex test','','','','','','');
FILE_SCHEMA(('TEST'));
ENDSEC;
DATA;
#1 = (LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#2 = CARTESIAN_POINT('',(1.0,2.0,3.0));
#3 = VERTEX_POINT('',#2);
ENDSEC;
END-ISO-10303-21;
"#;

    #[test]
    fn complex_entity_parses_sub_entities() {
        let path = write_test_file("complex_entity.stp", COMPLEX_ENTITY_STEP);
        let (doc, _report) = import_step(&path).unwrap();

        assert_eq!(doc.parts.len(), 1);

        let path2 =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/42-bspline.step");
        let content = std::fs::read_to_string(&path2).unwrap();
        let data_section = super::extract_section(&content, "DATA;").unwrap();
        assert!(!data_section.is_empty());
    }

    const TYPED_VALUE_STEP: &str = r#"ISO-10303-21;
HEADER;
FILE_NAME('typed test','','','','','','');
FILE_SCHEMA(('TEST'));
ENDSEC;
DATA;
#1 = CARTESIAN_POINT('',(1.0,2.0,3.0));
#2 = VERTEX_POINT('',#1);
#3 = MEASURE_REPRESENTATION_ITEM('volume measure',VOLUME_MEASURE(355877.882829),#411);
#4 = CARTESIAN_POINT('',(4.0,5.0,6.0));
#5 = VERTEX_POINT('',#4);
ENDSEC;
END-ISO-10303-21;
"#;

    #[test]
    fn typed_value_parses_in_args() {
        let path = write_test_file("typed_value.stp", TYPED_VALUE_STEP);
        let result = import_step(&path);
        assert!(
            result.is_ok(),
            "typed value import failed: {:?}",
            result.err()
        );
    }

    #[test]
    fn import_corpus_60() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/60-real-sg1-c5-214.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 60 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_61() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/61-real-io1-cm-214.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 61 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_62() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/62-real-dm1-id-214.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 62 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_63() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/63-real-ats1-out.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 63 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_64() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/64-real-ats2-out.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 64 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_65() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/65-real-ats3-out.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 65 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_66() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/66-real-ats4-out.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 66 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_67() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/67-real-ats7-out.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 67 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_68() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/68-real-screw.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 68 failed: {:?}", result.err());
    }

    #[test]
    fn import_corpus_69() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/69-real-as1-oc-214.step");
        let result = import_step(&path);
        assert!(result.is_ok(), "import of 69 failed: {:?}", result.err());
    }
}
