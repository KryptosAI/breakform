use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Length,
    Angle,
    Mass,
    Pressure,
    Density,
    Temperature,
    ThermalConductivity,
    Dimensionless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    #[serde(rename = "mm")]
    Millimeter,
    #[serde(rename = "cm")]
    Centimeter,
    #[serde(rename = "m")]
    Meter,
    #[serde(rename = "inch")]
    Inch,
    #[serde(rename = "deg")]
    Degree,
    #[serde(rename = "rad")]
    Radian,
    #[serde(rename = "kg")]
    Kilogram,
    #[serde(rename = "Pa")]
    Pascal,
    #[serde(rename = "MPa")]
    Megapascal,
    #[serde(rename = "GPa")]
    Gigapascal,
    #[serde(rename = "kg/m3")]
    KilogramPerCubicMeter,
    #[serde(rename = "K")]
    Kelvin,
    #[serde(rename = "W/m.K")]
    WattPerMeterKelvin,
    #[serde(rename = "1")]
    Dimensionless,
}

impl Unit {
    pub fn dimension(&self) -> Dimension {
        use Unit::*;
        match self {
            Millimeter | Centimeter | Meter | Inch => Dimension::Length,
            Degree | Radian => Dimension::Angle,
            Kilogram => Dimension::Mass,
            Pascal | Megapascal | Gigapascal => Dimension::Pressure,
            KilogramPerCubicMeter => Dimension::Density,
            Kelvin => Dimension::Temperature,
            WattPerMeterKelvin => Dimension::ThermalConductivity,
            Unit::Dimensionless => Dimension::Dimensionless,
        }
    }

    /// Multiplier converting a value in this unit to SI base.
    pub fn to_si_factor(&self) -> f64 {
        use Unit::*;
        match self {
            Millimeter => 1e-3,
            Centimeter => 1e-2,
            Meter => 1.0,
            Inch => 0.0254,
            Degree => std::f64::consts::PI / 180.0,
            Radian => 1.0,
            Kilogram => 1.0,
            Pascal => 1.0,
            Megapascal => 1e6,
            Gigapascal => 1e9,
            KilogramPerCubicMeter => 1.0,
            Kelvin => 1.0,
            WattPerMeterKelvin => 1.0,
            Unit::Dimensionless => 1.0,
        }
    }

    pub fn symbol(&self) -> &'static str {
        use Unit::*;
        match self {
            Millimeter => "mm",
            Centimeter => "cm",
            Meter => "m",
            Inch => "inch",
            Degree => "deg",
            Radian => "rad",
            Kilogram => "kg",
            Pascal => "Pa",
            Megapascal => "MPa",
            Gigapascal => "GPa",
            KilogramPerCubicMeter => "kg/m3",
            Kelvin => "K",
            WattPerMeterKelvin => "W/m.K",
            Unit::Dimensionless => "1",
        }
    }
}

impl FromStr for Unit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Unit::*;
        Ok(match s {
            "mm" => Millimeter,
            "cm" => Centimeter,
            "m" => Meter,
            "in" | "inch" => Inch,
            "deg" | "degree" => Degree,
            "rad" | "radian" => Radian,
            "kg" => Kilogram,
            "Pa" => Pascal,
            "MPa" => Megapascal,
            "GPa" => Gigapascal,
            "kg/m3" | "kg/m^3" => KilogramPerCubicMeter,
            "K" => Kelvin,
            "W/m.K" | "W/(m.K)" => WattPerMeterKelvin,
            "1" | "" => Unit::Dimensionless,
            other => return Err(format!("unknown unit: {other}")),
        })
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// A dimensioned scalar. Serialization without a unit is impossible by construction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    pub value: f64,
    pub unit: Unit,
}

impl Quantity {
    pub fn new(value: f64, unit: Unit) -> Self {
        Quantity { value, unit }
    }

    pub fn to_si(&self) -> f64 {
        self.value * self.unit.to_si_factor()
    }

    pub fn convert_to(&self, target: Unit) -> Result<Quantity, String> {
        if self.unit.dimension() != target.dimension() {
            return Err(format!(
                "dimension mismatch: {} -> {}",
                self.unit, target
            ));
        }
        Ok(Quantity {
            value: self.to_si() / target.to_si_factor(),
            unit: target,
        })
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.value, self.unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inch_to_mm() {
        let q = Quantity::new(1.0, Unit::Inch);
        let mm = q.convert_to(Unit::Millimeter).unwrap();
        assert!((mm.value - 25.4).abs() < 1e-9);
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let q = Quantity::new(1.0, Unit::Inch);
        assert!(q.convert_to(Unit::Pascal).is_err());
    }

    #[test]
    fn parse_symbols() {
        assert_eq!("mm".parse::<Unit>().unwrap(), Unit::Millimeter);
        assert_eq!("GPa".parse::<Unit>().unwrap(), Unit::Gigapascal);
        assert!("furlong".parse::<Unit>().is_err());
    }
}
