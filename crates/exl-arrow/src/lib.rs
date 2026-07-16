use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::{Array, ArrayRef, Float32Array, ListArray, StructArray, UInt32Array};
use arrow::buffer::OffsetBuffer;
use arrow::datatypes::{DataType, Field, Fields, Schema};
use arrow::record_batch::RecordBatch;
use exl_core::geom::Mesh;

fn vertices_list_type() -> DataType {
    DataType::List(Arc::new(Field::new(
        "item",
        DataType::Struct(Fields::from(vec![
            Field::new("x", DataType::Float32, false),
            Field::new("y", DataType::Float32, false),
            Field::new("z", DataType::Float32, false),
        ])),
        false,
    )))
}

fn faces_list_type() -> DataType {
    DataType::List(Arc::new(Field::new("item", DataType::UInt32, false)))
}

fn normals_list_type() -> DataType {
    DataType::List(Arc::new(Field::new(
        "item",
        DataType::Struct(Fields::from(vec![
            Field::new("x", DataType::Float32, false),
            Field::new("y", DataType::Float32, false),
            Field::new("z", DataType::Float32, false),
        ])),
        false,
    )))
}

fn uvs_list_type() -> DataType {
    DataType::List(Arc::new(Field::new(
        "item",
        DataType::Struct(Fields::from(vec![
            Field::new("u", DataType::Float32, false),
            Field::new("v", DataType::Float32, false),
        ])),
        false,
    )))
}

fn face_groups_list_type() -> DataType {
    DataType::List(Arc::new(Field::new("item", DataType::UInt32, false)))
}

fn make_f32x3_struct(data: &[[f32; 3]]) -> StructArray {
    let x: Vec<f32> = data.iter().map(|v| v[0]).collect();
    let y: Vec<f32> = data.iter().map(|v| v[1]).collect();
    let z: Vec<f32> = data.iter().map(|v| v[2]).collect();
    StructArray::new(
        Fields::from(vec![
            Field::new("x", DataType::Float32, false),
            Field::new("y", DataType::Float32, false),
            Field::new("z", DataType::Float32, false),
        ]),
        vec![
            Arc::new(Float32Array::from(x)) as ArrayRef,
            Arc::new(Float32Array::from(y)) as ArrayRef,
            Arc::new(Float32Array::from(z)) as ArrayRef,
        ],
        None,
    )
}

fn make_empty_f32x3_struct() -> StructArray {
    StructArray::new(
        Fields::from(vec![
            Field::new("x", DataType::Float32, false),
            Field::new("y", DataType::Float32, false),
            Field::new("z", DataType::Float32, false),
        ]),
        vec![
            Arc::new(Float32Array::from(Vec::<f32>::new())) as ArrayRef,
            Arc::new(Float32Array::from(Vec::<f32>::new())) as ArrayRef,
            Arc::new(Float32Array::from(Vec::<f32>::new())) as ArrayRef,
        ],
        None,
    )
}

fn make_f32x2_struct(data: &[[f32; 2]]) -> StructArray {
    let u: Vec<f32> = data.iter().map(|v| v[0]).collect();
    let v: Vec<f32> = data.iter().map(|v| v[1]).collect();
    StructArray::new(
        Fields::from(vec![
            Field::new("u", DataType::Float32, false),
            Field::new("v", DataType::Float32, false),
        ]),
        vec![
            Arc::new(Float32Array::from(u)) as ArrayRef,
            Arc::new(Float32Array::from(v)) as ArrayRef,
        ],
        None,
    )
}

fn make_empty_f32x2_struct() -> StructArray {
    StructArray::new(
        Fields::from(vec![
            Field::new("u", DataType::Float32, false),
            Field::new("v", DataType::Float32, false),
        ]),
        vec![
            Arc::new(Float32Array::from(Vec::<f32>::new())) as ArrayRef,
            Arc::new(Float32Array::from(Vec::<f32>::new())) as ArrayRef,
        ],
        None,
    )
}

fn make_list(item_field: Field, child: ArrayRef) -> ListArray {
    let offsets = OffsetBuffer::<i32>::from_lengths(vec![child.len()]);
    ListArray::new(Arc::new(item_field), offsets, child, None)
}

fn extract_f32x3_struct(arr: &StructArray) -> Vec<[f32; 3]> {
    let x_arr = arr
        .column(0)
        .as_any()
        .downcast_ref::<Float32Array>()
        .expect("x is Float32Array");
    let y_arr = arr
        .column(1)
        .as_any()
        .downcast_ref::<Float32Array>()
        .expect("y is Float32Array");
    let z_arr = arr
        .column(2)
        .as_any()
        .downcast_ref::<Float32Array>()
        .expect("z is Float32Array");
    (0..arr.len())
        .map(|i| [x_arr.value(i), y_arr.value(i), z_arr.value(i)])
        .collect()
}

fn extract_f32x2_struct(arr: &StructArray) -> Vec<[f32; 2]> {
    let u_arr = arr
        .column(0)
        .as_any()
        .downcast_ref::<Float32Array>()
        .expect("u is Float32Array");
    let v_arr = arr
        .column(1)
        .as_any()
        .downcast_ref::<Float32Array>()
        .expect("v is Float32Array");
    (0..arr.len())
        .map(|i| [u_arr.value(i), v_arr.value(i)])
        .collect()
}

pub fn mesh_to_record_batches(meshes: &[(usize, &Mesh)]) -> Vec<RecordBatch> {
    meshes
        .iter()
        .map(|(part_idx, mesh)| {
            let v_struct = make_f32x3_struct(&mesh.vertices);
            let v_list: ArrayRef = Arc::new(make_list(
                Field::new("item", v_struct.data_type().clone(), false),
                Arc::new(v_struct),
            ));

            let flat_faces: Vec<u32> =
                mesh.faces.iter().flat_map(|f| [f[0], f[1], f[2]]).collect();
            let faces_list: ArrayRef = Arc::new(make_list(
                Field::new("item", DataType::UInt32, false),
                Arc::new(UInt32Array::from(flat_faces)),
            ));

            let n_inner: Arc<dyn Array> = match &mesh.normals {
                Some(data) => Arc::new(make_f32x3_struct(data)),
                None => Arc::new(make_empty_f32x3_struct()),
            };
            let n_list: ArrayRef = Arc::new(make_list(
                Field::new("item", n_inner.data_type().clone(), false),
                n_inner,
            ));

            let uv_inner: Arc<dyn Array> = match &mesh.uvs {
                Some(data) => Arc::new(make_f32x2_struct(data)),
                None => Arc::new(make_empty_f32x2_struct()),
            };
            let uv_list: ArrayRef = Arc::new(make_list(
                Field::new("item", uv_inner.data_type().clone(), false),
                uv_inner,
            ));

            let fg_inner: Arc<dyn Array> = match &mesh.face_groups {
                Some(ref data) => Arc::new(UInt32Array::from(data.clone())),
                None => Arc::new(UInt32Array::from(Vec::<u32>::new())),
            };
            let fg_list: ArrayRef = Arc::new(make_list(
                Field::new("item", DataType::UInt32, false),
                fg_inner,
            ));

            let mut metadata = HashMap::new();
            metadata.insert("exl_part_index".to_string(), part_idx.to_string());
            metadata.insert(
                "exl_group_names".to_string(),
                serde_json::to_string(&mesh.group_names).unwrap(),
            );
            metadata.insert("exl_faces_stride".to_string(), "3".to_string());

            let schema = Arc::new(Schema::new_with_metadata(
                vec![
                    Field::new("vertices", vertices_list_type(), false),
                    Field::new("faces", faces_list_type(), false),
                    Field::new("normals", normals_list_type(), true),
                    Field::new("uvs", uvs_list_type(), true),
                    Field::new("face_groups", face_groups_list_type(), true),
                ],
                metadata,
            ));

            RecordBatch::try_new(schema, vec![v_list, faces_list, n_list, uv_list, fg_list])
                .expect("RecordBatch construction must succeed")
        })
        .collect()
}

fn unwrap_list_values(arr: &dyn Array, col_name: &str) -> ArrayRef {
    let list = arr
        .as_any()
        .downcast_ref::<ListArray>()
        .unwrap_or_else(|| panic!("column {} is not ListArray", col_name));
    if list.len() == 0 {
        panic!("empty batch for column {}", col_name);
    }
    list.value(0)
}

pub fn record_batches_to_meshes(batches: &[RecordBatch]) -> Vec<(usize, Mesh)> {
    batches
        .iter()
        .map(|batch| {
            let schema = batch.schema();
            let metadata = schema.metadata();

            let part_idx: usize = metadata
                .get("exl_part_index")
                .and_then(|v| v.parse().ok())
                .expect("missing exl_part_index metadata");

            let group_names: Vec<String> = metadata
                .get("exl_group_names")
                .map(|v| serde_json::from_str(v).unwrap_or_default())
                .unwrap_or_default();

            let v_inner = unwrap_list_values(batch.column(0).as_ref(), "vertices");
            let vertices_arr = v_inner
                .as_any()
                .downcast_ref::<StructArray>()
                .expect("vertices inner is StructArray");
            let vertices = extract_f32x3_struct(vertices_arr);

            let f_inner = unwrap_list_values(batch.column(1).as_ref(), "faces");
            let faces_arr = f_inner
                .as_any()
                .downcast_ref::<UInt32Array>()
                .expect("faces inner is UInt32Array");
            let faces: Vec<[u32; 3]> = faces_arr
                .values()
                .chunks_exact(3)
                .map(|c| [c[0], c[1], c[2]])
                .collect();

            let n_inner = unwrap_list_values(batch.column(2).as_ref(), "normals");
            let normals_arr = n_inner
                .as_any()
                .downcast_ref::<StructArray>()
                .expect("normals inner is StructArray");
            let normals = if normals_arr.len() > 0 {
                Some(extract_f32x3_struct(normals_arr))
            } else {
                None
            };

            let uv_inner = unwrap_list_values(batch.column(3).as_ref(), "uvs");
            let uvs_arr = uv_inner
                .as_any()
                .downcast_ref::<StructArray>()
                .expect("uvs inner is StructArray");
            let uvs = if uvs_arr.len() > 0 {
                Some(extract_f32x2_struct(uvs_arr))
            } else {
                None
            };

            let fg_inner = unwrap_list_values(batch.column(4).as_ref(), "face_groups");
            let fg_arr = fg_inner
                .as_any()
                .downcast_ref::<UInt32Array>()
                .expect("face_groups inner is UInt32Array");
            let face_groups = if fg_arr.len() > 0 {
                Some(fg_arr.values().to_vec())
            } else {
                None
            };

            (
                part_idx,
                Mesh {
                    vertices,
                    faces,
                    normals,
                    uvs,
                    face_groups,
                    group_names,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_simple() {
        let mesh = Mesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![[0, 1, 2]],
            ..Default::default()
        };

        let batches = mesh_to_record_batches(&[(0, &mesh)]);
        assert_eq!(batches.len(), 1);
        let result = record_batches_to_meshes(&batches);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 0);
        assert_eq!(result[0].1, mesh);
    }

    #[test]
    fn round_trip_with_all_optionals() {
        let mesh = Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            faces: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            normals: Some(vec![
                [0.0, 0.0, -1.0],
                [0.0, -1.0, 0.0],
                [-1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
            ]),
            uvs: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]),
            face_groups: Some(vec![0, 0, 1, 1]),
            group_names: vec!["group_a".into(), "group_b".into()],
        };

        let batches = mesh_to_record_batches(&[(2, &mesh)]);
        assert_eq!(batches.len(), 1);
        let result = record_batches_to_meshes(&batches);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 2);
        assert_eq!(result[0].1, mesh);
    }

    #[test]
    fn round_trip_two_meshes() {
        let m0 = Mesh {
            vertices: vec![[0.0; 3]],
            faces: vec![[0, 0, 0]],
            ..Default::default()
        };
        let m1 = Mesh {
            vertices: vec![[1.0; 3]],
            faces: vec![[0, 0, 0]],
            normals: Some(vec![[0.0, 1.0, 0.0]]),
            ..Default::default()
        };

        let batches = mesh_to_record_batches(&[(0, &m0), (1, &m1)]);
        assert_eq!(batches.len(), 2);
        let result = record_batches_to_meshes(&batches);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, 0);
        assert_eq!(result[0].1, m0);
        assert_eq!(result[1].0, 1);
        assert_eq!(result[1].1, m1);
    }

    #[test]
    fn round_trip_empty_mesh() {
        let mesh = Mesh::default();
        let batches = mesh_to_record_batches(&[(3, &mesh)]);
        assert_eq!(batches.len(), 1);
        let result = record_batches_to_meshes(&batches);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 3);
        assert_eq!(result[0].1, mesh);
    }
}
