//! Unit conversion system
//!
//! Handles conversions between different units of measurement.
//! Returns OperationResult with Veto for errors instead of Result.

use super::result::OperationResult;
use crate::{ConversionTarget, LiteralValue, NumericUnit};
use rust_decimal::Decimal;

/// Convert a value to a target unit (for `in` operator).
///
/// - Unit -> Number: extracts numeric value in target unit
/// - Number -> Unit: creates a unit with that value
///
/// Returns OperationResult with Veto for errors.
pub fn convert_unit(value: &LiteralValue, target: &ConversionTarget) -> OperationResult {
    match value {
        LiteralValue::Unit(unit) => {
            let converted_value = match (unit, target) {
                (NumericUnit::Duration(v, from), ConversionTarget::Duration(to)) => {
                    match convert_duration(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Mass(v, from), ConversionTarget::Mass(to)) => {
                    match convert_mass(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Length(v, from), ConversionTarget::Length(to)) => {
                    match convert_length(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Temperature(v, from), ConversionTarget::Temperature(to)) => {
                    match convert_temperature(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Power(v, from), ConversionTarget::Power(to)) => {
                    match convert_power(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Volume(v, from), ConversionTarget::Volume(to)) => {
                    match convert_volume(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Force(v, from), ConversionTarget::Force(to)) => {
                    match convert_force(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Pressure(v, from), ConversionTarget::Pressure(to)) => {
                    match convert_pressure(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Energy(v, from), ConversionTarget::Energy(to)) => {
                    match convert_energy(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Frequency(v, from), ConversionTarget::Frequency(to)) => {
                    match convert_frequency(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                (NumericUnit::Data(v, from), ConversionTarget::Data(to)) => {
                    match convert_data_size(*v, from, to) {
                        Ok(val) => val,
                        Err(msg) => return OperationResult::Veto(Some(msg)),
                    }
                }
                _ => {
                    return OperationResult::Veto(Some(
                        "Mismatched unit type for conversion".to_string(),
                    ));
                }
            };
            OperationResult::Value(LiteralValue::Number(converted_value))
        }

        LiteralValue::Number(n) => {
            let result = match target {
                ConversionTarget::Mass(u) => LiteralValue::Unit(NumericUnit::Mass(*n, u.clone())),
                ConversionTarget::Length(u) => {
                    LiteralValue::Unit(NumericUnit::Length(*n, u.clone()))
                }
                ConversionTarget::Volume(u) => {
                    LiteralValue::Unit(NumericUnit::Volume(*n, u.clone()))
                }
                ConversionTarget::Duration(u) => {
                    LiteralValue::Unit(NumericUnit::Duration(*n, u.clone()))
                }
                ConversionTarget::Temperature(u) => {
                    LiteralValue::Unit(NumericUnit::Temperature(*n, u.clone()))
                }
                ConversionTarget::Power(u) => LiteralValue::Unit(NumericUnit::Power(*n, u.clone())),
                ConversionTarget::Force(u) => LiteralValue::Unit(NumericUnit::Force(*n, u.clone())),
                ConversionTarget::Pressure(u) => {
                    LiteralValue::Unit(NumericUnit::Pressure(*n, u.clone()))
                }
                ConversionTarget::Energy(u) => {
                    LiteralValue::Unit(NumericUnit::Energy(*n, u.clone()))
                }
                ConversionTarget::Frequency(u) => {
                    LiteralValue::Unit(NumericUnit::Frequency(*n, u.clone()))
                }
                ConversionTarget::Data(u) => LiteralValue::Unit(NumericUnit::Data(*n, u.clone())),
                ConversionTarget::Percentage => LiteralValue::Percentage(n * Decimal::from(100)),
            };
            OperationResult::Value(result)
        }

        _ => OperationResult::Veto(Some("Cannot convert value to target".to_string())),
    }
}

type ConversionResult = Result<Decimal, String>;

/// Convert a NumericUnit value to its base unit value for comparison purposes.
/// Returns the value in the base unit (grams for mass, meters for length, etc.)
pub fn to_base_unit_value(unit: &crate::NumericUnit) -> Decimal {
    match unit {
        crate::NumericUnit::Mass(v, u) => mass_to_grams(*v, u),
        crate::NumericUnit::Length(v, u) => length_to_meters(*v, u),
        crate::NumericUnit::Duration(v, u) => duration_to_minutes(*v, u),
        crate::NumericUnit::Volume(v, u) => volume_to_liters(*v, u),
        crate::NumericUnit::Temperature(v, u) => temperature_to_celsius(*v, u),
        crate::NumericUnit::Power(v, u) => power_to_watts(*v, u),
        crate::NumericUnit::Force(v, u) => force_to_newtons(*v, u),
        crate::NumericUnit::Pressure(v, u) => pressure_to_pascals(*v, u),
        crate::NumericUnit::Energy(v, u) => energy_to_joules(*v, u),
        crate::NumericUnit::Frequency(v, u) => frequency_to_hertz(*v, u),
        crate::NumericUnit::Data(v, u) => data_to_bytes(*v, u),
    }
}

fn mass_to_grams(value: Decimal, from: &crate::MassUnit) -> Decimal {
    match from {
        crate::MassUnit::Gram => value,
        crate::MassUnit::Milligram => value / Decimal::from(1000),
        crate::MassUnit::Kilogram => value * Decimal::from(1000),
        crate::MassUnit::Ton => value * Decimal::from(1000000),
        crate::MassUnit::Pound => value * Decimal::new(45359237, 5),
        crate::MassUnit::Ounce => value * Decimal::new(2834952, 5),
    }
}

fn length_to_meters(value: Decimal, from: &crate::LengthUnit) -> Decimal {
    match from {
        crate::LengthUnit::Meter => value,
        crate::LengthUnit::Kilometer => value * Decimal::from(1000),
        crate::LengthUnit::Decimeter => value / Decimal::from(10),
        crate::LengthUnit::Centimeter => value / Decimal::from(100),
        crate::LengthUnit::Millimeter => value / Decimal::from(1000),
        crate::LengthUnit::Foot => value * Decimal::new(3048, 4),
        crate::LengthUnit::Inch => value * Decimal::new(254, 4),
        crate::LengthUnit::Yard => value * Decimal::new(9144, 4),
        crate::LengthUnit::Mile => value * Decimal::new(1609344, 3),
        crate::LengthUnit::NauticalMile => value * Decimal::from(1852),
    }
}

fn duration_to_minutes(value: Decimal, from: &crate::DurationUnit) -> Decimal {
    match from {
        crate::DurationUnit::Minute => value,
        crate::DurationUnit::Second => value / Decimal::from(60),
        crate::DurationUnit::Millisecond => value / Decimal::from(60000),
        crate::DurationUnit::Microsecond => value / Decimal::from(60000000),
        crate::DurationUnit::Hour => value * Decimal::from(60),
        crate::DurationUnit::Day => value * Decimal::from(1440),
        crate::DurationUnit::Week => value * Decimal::from(10080),
        crate::DurationUnit::Month => value * Decimal::from(43200),
        crate::DurationUnit::Year => value * Decimal::from(525600),
    }
}

fn volume_to_liters(value: Decimal, from: &crate::VolumeUnit) -> Decimal {
    match from {
        crate::VolumeUnit::Liter => value,
        crate::VolumeUnit::Milliliter => value / Decimal::from(1000),
        crate::VolumeUnit::Centiliter => value / Decimal::from(100),
        crate::VolumeUnit::Deciliter => value / Decimal::from(10),
        crate::VolumeUnit::CubicMeter => value * Decimal::from(1000),
        crate::VolumeUnit::CubicCentimeter => value / Decimal::from(1000),
        crate::VolumeUnit::Gallon => value * Decimal::new(378541, 5),
        crate::VolumeUnit::Quart => value * Decimal::new(946353, 6),
        crate::VolumeUnit::Pint => value * Decimal::new(473176, 6),
        crate::VolumeUnit::FluidOunce => value * Decimal::new(29574, 6),
    }
}

fn temperature_to_celsius(value: Decimal, from: &crate::TemperatureUnit) -> Decimal {
    match from {
        crate::TemperatureUnit::Celsius => value,
        crate::TemperatureUnit::Fahrenheit => {
            (value - Decimal::from(32)) * Decimal::from(5) / Decimal::from(9)
        }
        crate::TemperatureUnit::Kelvin => value - Decimal::new(27315, 2),
    }
}

fn power_to_watts(value: Decimal, from: &crate::PowerUnit) -> Decimal {
    match from {
        crate::PowerUnit::Watt => value,
        crate::PowerUnit::Milliwatt => value / Decimal::from(1000),
        crate::PowerUnit::Kilowatt => value * Decimal::from(1000),
        crate::PowerUnit::Megawatt => value * Decimal::from(1000000),
        crate::PowerUnit::Horsepower => value * Decimal::new(7457, 1),
    }
}

fn force_to_newtons(value: Decimal, from: &crate::ForceUnit) -> Decimal {
    match from {
        crate::ForceUnit::Newton => value,
        crate::ForceUnit::Kilonewton => value * Decimal::from(1000),
        crate::ForceUnit::Lbf => value * Decimal::new(44482, 4),
    }
}

fn pressure_to_pascals(value: Decimal, from: &crate::PressureUnit) -> Decimal {
    match from {
        crate::PressureUnit::Pascal => value,
        crate::PressureUnit::Kilopascal => value * Decimal::from(1000),
        crate::PressureUnit::Megapascal => value * Decimal::from(1000000),
        crate::PressureUnit::Bar => value * Decimal::from(100000),
        crate::PressureUnit::Psi => value * Decimal::new(689476, 2),
        crate::PressureUnit::Atmosphere => value * Decimal::from(101325),
        crate::PressureUnit::Torr => value * Decimal::new(133322, 3),
        crate::PressureUnit::Mmhg => value * Decimal::new(133322, 3),
    }
}

fn energy_to_joules(value: Decimal, from: &crate::EnergyUnit) -> Decimal {
    match from {
        crate::EnergyUnit::Joule => value,
        crate::EnergyUnit::Kilojoule => value * Decimal::from(1000),
        crate::EnergyUnit::Megajoule => value * Decimal::from(1000000),
        crate::EnergyUnit::Calorie => value * Decimal::new(4184, 3),
        crate::EnergyUnit::Kilocalorie => value * Decimal::new(4184, 0),
        crate::EnergyUnit::Watthour => value * Decimal::from(3600),
        crate::EnergyUnit::Kilowatthour => value * Decimal::from(3600000),
        crate::EnergyUnit::Btu => value * Decimal::new(1055, 0),
    }
}

fn frequency_to_hertz(value: Decimal, from: &crate::FrequencyUnit) -> Decimal {
    match from {
        crate::FrequencyUnit::Hertz => value,
        crate::FrequencyUnit::Kilohertz => value * Decimal::from(1000),
        crate::FrequencyUnit::Megahertz => value * Decimal::from(1000000),
        crate::FrequencyUnit::Gigahertz => value * Decimal::from(1000000000i64),
    }
}

fn data_to_bytes(value: Decimal, from: &crate::DataUnit) -> Decimal {
    match from {
        crate::DataUnit::Byte => value,
        crate::DataUnit::Kilobyte => value * Decimal::from(1000),
        crate::DataUnit::Megabyte => value * Decimal::from(1000000),
        crate::DataUnit::Gigabyte => value * Decimal::from(1000000000i64),
        crate::DataUnit::Terabyte => value * Decimal::from(1000000000000i64),
        crate::DataUnit::Petabyte => value * Decimal::from(1000000000000000i64),
        crate::DataUnit::Kibibyte => value * Decimal::from(1024),
        crate::DataUnit::Mebibyte => value * Decimal::from(1048576),
        crate::DataUnit::Gibibyte => value * Decimal::from(1073741824i64),
        crate::DataUnit::Tebibyte => value * Decimal::from(1099511627776i64),
    }
}

fn convert_mass(value: Decimal, from: &crate::MassUnit, to: &crate::MassUnit) -> ConversionResult {
    if from == to {
        return Ok(value);
    }

    let grams = match from {
        crate::MassUnit::Gram => value,
        crate::MassUnit::Milligram => value / Decimal::from(1000),
        crate::MassUnit::Kilogram => value * Decimal::from(1000),
        crate::MassUnit::Ton => value * Decimal::from(1000000),
        crate::MassUnit::Pound => value * Decimal::new(45359237, 5),
        crate::MassUnit::Ounce => value * Decimal::new(2834952, 5),
    };

    let result = match to {
        crate::MassUnit::Gram => grams,
        crate::MassUnit::Milligram => grams * Decimal::from(1000),
        crate::MassUnit::Kilogram => grams / Decimal::from(1000),
        crate::MassUnit::Ton => grams / Decimal::from(1000000),
        crate::MassUnit::Pound => grams / Decimal::new(45359237, 5),
        crate::MassUnit::Ounce => grams / Decimal::new(2834952, 5),
    };

    Ok(result)
}

fn convert_length(
    value: Decimal,
    from: &crate::LengthUnit,
    to: &crate::LengthUnit,
) -> ConversionResult {
    if from == to {
        return Ok(value);
    }

    let meters = match from {
        crate::LengthUnit::Meter => value,
        crate::LengthUnit::Kilometer => value * Decimal::from(1000),
        crate::LengthUnit::Decimeter => value / Decimal::from(10),
        crate::LengthUnit::Centimeter => value / Decimal::from(100),
        crate::LengthUnit::Millimeter => value / Decimal::from(1000),
        crate::LengthUnit::Foot => value * Decimal::new(3048, 4),
        crate::LengthUnit::Inch => value * Decimal::new(254, 4),
        crate::LengthUnit::Yard => value * Decimal::new(9144, 4),
        crate::LengthUnit::Mile => value * Decimal::new(1609344, 3),
        crate::LengthUnit::NauticalMile => value * Decimal::from(1852),
    };

    let result = match to {
        crate::LengthUnit::Meter => meters,
        crate::LengthUnit::Kilometer => meters / Decimal::from(1000),
        crate::LengthUnit::Decimeter => meters * Decimal::from(10),
        crate::LengthUnit::Centimeter => meters * Decimal::from(100),
        crate::LengthUnit::Millimeter => meters * Decimal::from(1000),
        crate::LengthUnit::Foot => meters / Decimal::new(3048, 4),
        crate::LengthUnit::Inch => meters / Decimal::new(254, 4),
        crate::LengthUnit::Yard => meters / Decimal::new(9144, 4),
        crate::LengthUnit::Mile => meters / Decimal::new(1609344, 3),
        crate::LengthUnit::NauticalMile => meters / Decimal::from(1852),
    };

    Ok(result)
}

fn convert_duration(
    value: Decimal,
    from: &crate::DurationUnit,
    to: &crate::DurationUnit,
) -> ConversionResult {
    if from == to {
        return Ok(value);
    }

    if matches!(from, crate::DurationUnit::Month | crate::DurationUnit::Year)
        || matches!(to, crate::DurationUnit::Month | crate::DurationUnit::Year)
    {
        return Err(
            "Cannot convert calendar units (month/year) to other duration units. Use date arithmetic instead.".to_string()
        );
    }

    let seconds = crate::parsing::units::duration_to_seconds(value, from);

    let result = match to {
        crate::DurationUnit::Second => seconds,
        crate::DurationUnit::Minute => seconds / Decimal::from(60),
        crate::DurationUnit::Hour => seconds / Decimal::from(3600),
        crate::DurationUnit::Day => seconds / Decimal::from(86400),
        crate::DurationUnit::Week => seconds / Decimal::from(604800),
        crate::DurationUnit::Millisecond => seconds * Decimal::from(1000),
        crate::DurationUnit::Microsecond => seconds * Decimal::from(1000000),
        crate::DurationUnit::Month | crate::DurationUnit::Year => {
            return Err("Internal error: Calendar units should not reach here".to_string());
        }
    };

    Ok(result)
}

fn convert_temperature(
    value: Decimal,
    from: &crate::TemperatureUnit,
    to: &crate::TemperatureUnit,
) -> ConversionResult {
    if from == to {
        return Ok(value);
    }

    let celsius = match from {
        crate::TemperatureUnit::Celsius => value,
        crate::TemperatureUnit::Fahrenheit => {
            (value - Decimal::from(32)) * Decimal::new(5, 0) / Decimal::new(9, 0)
        }
        crate::TemperatureUnit::Kelvin => value - Decimal::new(27315, 2),
    };

    let result = match to {
        crate::TemperatureUnit::Celsius => celsius,
        crate::TemperatureUnit::Fahrenheit => {
            celsius * Decimal::new(9, 0) / Decimal::new(5, 0) + Decimal::from(32)
        }
        crate::TemperatureUnit::Kelvin => celsius + Decimal::new(27315, 2),
    };

    Ok(result)
}

fn convert_power(
    value: Decimal,
    from: &crate::PowerUnit,
    to: &crate::PowerUnit,
) -> ConversionResult {
    if from == to {
        return Ok(value);
    }

    let watts = match from {
        crate::PowerUnit::Watt => value,
        crate::PowerUnit::Kilowatt => value * Decimal::from(1000),
        crate::PowerUnit::Megawatt => value * Decimal::from(1000000),
        crate::PowerUnit::Milliwatt => value / Decimal::from(1000),
        crate::PowerUnit::Horsepower => value * Decimal::new(7457, 1),
    };

    let result = match to {
        crate::PowerUnit::Watt => watts,
        crate::PowerUnit::Kilowatt => watts / Decimal::from(1000),
        crate::PowerUnit::Megawatt => watts / Decimal::from(1000000),
        crate::PowerUnit::Milliwatt => watts * Decimal::from(1000),
        crate::PowerUnit::Horsepower => watts / Decimal::new(7457, 1),
    };

    Ok(result)
}

fn convert_volume(
    value: Decimal,
    from: &crate::VolumeUnit,
    to: &crate::VolumeUnit,
) -> ConversionResult {
    use crate::VolumeUnit::*;
    if from == to {
        return Ok(value);
    }

    let liters = match from {
        Liter => value,
        Milliliter => value / Decimal::from(1000),
        Centiliter => value / Decimal::from(100),
        Deciliter => value / Decimal::from(10),
        CubicMeter => value * Decimal::from(1000),
        CubicCentimeter => value / Decimal::from(1000),
        Gallon => value * Decimal::new(3785411784, 9),
        Quart => value * Decimal::new(946352946, 9),
        Pint => value * Decimal::new(473176473, 9),
        FluidOunce => value * Decimal::new(2957352956, 11),
    };

    let result = match to {
        Liter => liters,
        Milliliter => liters * Decimal::from(1000),
        Centiliter => liters * Decimal::from(100),
        Deciliter => liters * Decimal::from(10),
        CubicMeter => liters / Decimal::from(1000),
        CubicCentimeter => liters * Decimal::from(1000),
        Gallon => liters / Decimal::new(3785411784, 9),
        Quart => liters / Decimal::new(946352946, 9),
        Pint => liters / Decimal::new(473176473, 9),
        FluidOunce => liters / Decimal::new(2957352956, 11),
    };

    Ok(result)
}

fn convert_force(
    value: Decimal,
    from: &crate::ForceUnit,
    to: &crate::ForceUnit,
) -> ConversionResult {
    use crate::ForceUnit::*;
    if from == to {
        return Ok(value);
    }

    let newtons = match from {
        Newton => value,
        Kilonewton => value * Decimal::from(1000),
        Lbf => value * Decimal::new(44482, 5),
    };

    let result = match to {
        Newton => newtons,
        Kilonewton => newtons / Decimal::from(1000),
        Lbf => newtons / Decimal::new(44482, 5),
    };

    Ok(result)
}

fn convert_pressure(
    value: Decimal,
    from: &crate::PressureUnit,
    to: &crate::PressureUnit,
) -> ConversionResult {
    use crate::PressureUnit::*;
    if from == to {
        return Ok(value);
    }

    let pascals = match from {
        Pascal => value,
        Kilopascal => value * Decimal::from(1000),
        Megapascal => value * Decimal::from(1000000),
        Bar => value * Decimal::from(100000),
        Atmosphere => value * Decimal::new(101325, 0),
        Psi => value * Decimal::new(689476, 2),
        Torr => value * Decimal::new(13332237, 5),
        Mmhg => value * Decimal::new(13332237, 5),
    };

    let result = match to {
        Pascal => pascals,
        Kilopascal => pascals / Decimal::from(1000),
        Megapascal => pascals / Decimal::from(1000000),
        Bar => pascals / Decimal::from(100000),
        Atmosphere => pascals / Decimal::new(101325, 0),
        Psi => pascals / Decimal::new(689476, 2),
        Torr => pascals / Decimal::new(13332237, 5),
        Mmhg => pascals / Decimal::new(13332237, 5),
    };

    Ok(result)
}

fn convert_energy(
    value: Decimal,
    from: &crate::EnergyUnit,
    to: &crate::EnergyUnit,
) -> ConversionResult {
    use crate::EnergyUnit::*;
    if from == to {
        return Ok(value);
    }

    let joules = match from {
        Joule => value,
        Kilojoule => value * Decimal::from(1000),
        Megajoule => value * Decimal::from(1000000),
        Watthour => value * Decimal::from(3600),
        Kilowatthour => value * Decimal::from(3600000),
        Calorie => value * Decimal::new(4184, 3),
        Kilocalorie => value * Decimal::new(4184, 0),
        Btu => value * Decimal::new(105506, 2),
    };

    let result = match to {
        Joule => joules,
        Kilojoule => joules / Decimal::from(1000),
        Megajoule => joules / Decimal::from(1000000),
        Watthour => joules / Decimal::from(3600),
        Kilowatthour => joules / Decimal::from(3600000),
        Calorie => joules / Decimal::new(4184, 3),
        Kilocalorie => joules / Decimal::new(4184, 0),
        Btu => joules / Decimal::new(105506, 2),
    };

    Ok(result)
}

fn convert_frequency(
    value: Decimal,
    from: &crate::FrequencyUnit,
    to: &crate::FrequencyUnit,
) -> ConversionResult {
    use crate::FrequencyUnit::*;
    if from == to {
        return Ok(value);
    }

    let hertz = match from {
        Hertz => value,
        Kilohertz => value * Decimal::from(1000),
        Megahertz => value * Decimal::from(1000000),
        Gigahertz => value * Decimal::from(1000000000i64),
    };

    let result = match to {
        Hertz => hertz,
        Kilohertz => hertz / Decimal::from(1000),
        Megahertz => hertz / Decimal::from(1000000),
        Gigahertz => hertz / Decimal::from(1000000000i64),
    };

    Ok(result)
}

fn convert_data_size(
    value: Decimal,
    from: &crate::DataUnit,
    to: &crate::DataUnit,
) -> ConversionResult {
    use crate::DataUnit::*;
    if from == to {
        return Ok(value);
    }

    let bytes = match from {
        Byte => value,
        Kilobyte => value * Decimal::from(1000),
        Megabyte => value * Decimal::from(1000000),
        Gigabyte => value * Decimal::from(1000000000i64),
        Terabyte => value * Decimal::from(1000000000000i64),
        Petabyte => value * Decimal::from(1000000000000000i64),
        Kibibyte => value * Decimal::from(1024),
        Mebibyte => value * Decimal::from(1048576),
        Gibibyte => value * Decimal::from(1073741824i64),
        Tebibyte => value * Decimal::from(1099511627776i64),
    };

    let result = match to {
        Byte => bytes,
        Kilobyte => bytes / Decimal::from(1000),
        Megabyte => bytes / Decimal::from(1000000),
        Gigabyte => bytes / Decimal::from(1000000000i64),
        Terabyte => bytes / Decimal::from(1000000000000i64),
        Petabyte => bytes / Decimal::from(1000000000000000i64),
        Kibibyte => bytes / Decimal::from(1024),
        Mebibyte => bytes / Decimal::from(1048576),
        Gibibyte => bytes / Decimal::from(1073741824i64),
        Tebibyte => bytes / Decimal::from(1099511627776i64),
    };

    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::{Engine, LiteralValue};
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::str::FromStr;

    #[test]
    fn test_mass_conversions() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc test1
fact weight_kg = 10 kilograms
rule weight_lbs = weight_kg in pounds
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test1", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("weight_lbs")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                let expected = Decimal::from_str("22.0462").unwrap();
                let diff = (amount - expected).abs();
                assert!(
                    diff < Decimal::from_str("0.01").unwrap(),
                    "Expected ~22.05, got {}",
                    amount
                );
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test1");

        engine
            .add_lemma_code(
                r#"
doc test2
fact weight_tons = 2 tons
rule weight_kg = weight_tons in kilograms
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test2", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("weight_kg")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("2000").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test2");
    }

    #[test]
    fn test_length_conversions() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc test1
fact distance_km = 100 kilometers
rule distance_miles = distance_km in miles
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test1", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("distance_miles")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert!(
                    (amount - Decimal::from_str("62.1371").unwrap()).abs()
                        < Decimal::from_str("0.01").unwrap(),
                    "Expected ~62.14, got {}",
                    amount
                );
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test1");

        engine
            .add_lemma_code(
                r#"
doc test2
fact distance_yards = 100 yards
rule distance_meters = distance_yards in meters
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test2", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("distance_meters")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert!(
                    (amount - Decimal::from_str("91.44").unwrap()).abs()
                        < Decimal::from_str("0.01").unwrap(),
                    "Expected ~91.44, got {}",
                    amount
                );
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test2");

        engine
            .add_lemma_code(
                r#"
doc test3
fact length_dm = 50 decimeters
rule length_cm = length_dm in centimeters
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test3", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("length_cm")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("500.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test3");
    }

    #[test]
    fn test_volume_conversions() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc test1
fact volume_l = 2 liters
rule volume_ml = volume_l in milliliters
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test1", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("volume_ml")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("2000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test1");

        engine
            .add_lemma_code(
                r#"
doc test2
fact volume_gal = 5 gallons
rule volume_l = volume_gal in liters
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test2", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("volume_l")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert!(
                    (amount - Decimal::from_str("18.927").unwrap()).abs()
                        < Decimal::from_str("0.01").unwrap(),
                    "Expected ~18.93 liters, got {}",
                    amount
                );
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test2");

        engine
            .add_lemma_code(
                r#"
doc test3
fact volume_qt = 8 quarts
rule volume_pt = volume_qt in pints
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test3", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("volume_pt")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("16.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test3");

        engine
            .add_lemma_code(
                r#"
doc test4
fact volume_cl = 75 centiliters
rule volume_dl = volume_cl in deciliters
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test4", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("volume_dl")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert!(
                    (amount - Decimal::from_str("7.5").unwrap()).abs()
                        < Decimal::from_str("0.01").unwrap(),
                    "Expected 7.5 deciliters, got {}",
                    amount
                );
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test4");
    }

    #[test]
    fn test_duration_conversions() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc test1
fact time_h = 3 hours
rule time_min = time_h in minutes
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test1", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("time_min")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(n) => {
                assert_eq!(n, &Decimal::from_str("180.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test1");

        engine
            .add_lemma_code(
                r#"
doc test2
fact time_s = 5 seconds
rule time_ms = time_s in milliseconds
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test2", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("time_ms")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(n) => {
                assert_eq!(n, &Decimal::from_str("5000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test2");

        engine
            .add_lemma_code(
                r#"
doc test3
fact time_ms = 1000 milliseconds
rule time_us = time_ms in microseconds
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test3", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("time_us")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(n) => {
                assert_eq!(n, &Decimal::from_str("1000000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test3");
    }

    #[test]
    fn test_power_conversions() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc test1
fact power_kw = 5 kilowatts
rule power_w = power_kw in watts
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test1", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("power_w")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("5000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test1");

        engine
            .add_lemma_code(
                r#"
doc test2
fact power_w = 2000 watts
rule power_mw = power_w in milliwatts
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test2", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("power_mw")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("2000000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test2");
    }

    #[test]
    fn test_energy_conversions() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc test1
fact energy_kj = 10 kilojoules
rule energy_j = energy_kj in joules
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test1", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("energy_j")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("10000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test1");

        engine
            .add_lemma_code(
                r#"
doc test2
fact energy_kwh = 2 kilowatthours
rule energy_wh = energy_kwh in watthours
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test2", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("energy_wh")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("2000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test2");

        engine
            .add_lemma_code(
                r#"
doc test3
fact energy_kcal = 5 kilocalories
rule energy_cal = energy_kcal in calories
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test3", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("energy_cal")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("5000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test3");
    }

    #[test]
    fn test_pressure_conversions() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc test1
fact pressure_kpa = 250 kilopascals
rule pressure_pa = pressure_kpa in pascals
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test1", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("pressure_pa")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("250000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test1");

        engine
            .add_lemma_code(
                r#"
doc test2
fact pressure_mpa = 3 megapascals
rule pressure_kpa = pressure_mpa in kilopascals
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test2", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("pressure_kpa")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("3000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test2");
    }

    #[test]
    fn test_datasize_conversions() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc test1
fact size_gb = 10 gigabytes
rule size_mb = size_gb in megabytes
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test1", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("size_mb")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("10000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test1");

        engine
            .add_lemma_code(
                r#"
doc test2
fact size_tb = 2 terabytes
rule size_gb = size_tb in gigabytes
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test2", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("size_gb")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("2000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test2");

        engine
            .add_lemma_code(
                r#"
doc test3
fact size_pb = 1 petabytes
rule size_tb = size_pb in terabytes
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test3", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("size_tb")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("1000.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test3");

        engine
            .add_lemma_code(
                r#"
doc test4
fact size_gib = 8 gibibytes
rule size_mib = size_gib in mebibytes
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test4", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("size_mib")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("8192.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test4");

        engine
            .add_lemma_code(
                r#"
doc test5
fact size_kib = 2048 kibibytes
rule size_b = size_kib in bytes
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("test5", vec![], HashMap::new()).unwrap();
        let result = response
            .results
            .get("size_b")
            .unwrap()
            .result
            .value()
            .unwrap();
        match result {
            LiteralValue::Number(amount) => {
                assert_eq!(amount, &Decimal::from_str("2097152.0").unwrap());
            }
            _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
        }
        engine.remove_document("test5");
    }

    #[test]
    fn test_complex_multi_unit_scenario() {
        let mut engine = Engine::new();

        engine
            .add_lemma_code(
                r#"
doc complex
fact base_price = 100
fact weight = 5 kilograms
fact distance = 1000 kilometers
fact discount = 10%

rule weight_lbs = weight in pounds
rule distance_miles = distance in miles
rule discounted_price = base_price * (1 - discount)
"#,
                "test.lemma",
            )
            .unwrap();
        let response = engine.evaluate("complex", vec![], HashMap::new()).unwrap();

        let weight_result = response
            .results
            .get("weight_lbs")
            .unwrap()
            .result
            .value()
            .unwrap();
        match weight_result {
            LiteralValue::Number(amount) => {
                assert!(
                    (amount - Decimal::from_str("11.0231").unwrap()).abs()
                        < Decimal::from_str("0.01").unwrap()
                );
            }
            _ => panic!("Expected Number (from 'in' conversion)"),
        }

        let distance_result = response
            .results
            .get("distance_miles")
            .unwrap()
            .result
            .value()
            .unwrap();
        match distance_result {
            LiteralValue::Number(amount) => {
                assert!(
                    (amount - Decimal::from_str("621.371").unwrap()).abs()
                        < Decimal::from_str("0.01").unwrap()
                );
            }
            _ => panic!("Expected Number (from 'in' conversion)"),
        }

        let price_result = response
            .results
            .get("discounted_price")
            .unwrap()
            .result
            .value()
            .unwrap();
        match price_result {
            LiteralValue::Number(n) => {
                assert_eq!(n, &Decimal::from_str("90.0").unwrap());
            }
            _ => panic!("Expected Number"),
        }
        engine.remove_document("complex");
    }
}
