//! Unit conversion system
//!
//! Handles conversions between different units of measurement:
//! - Duration (seconds, minutes, hours, days, weeks, months, years)
//! - Mass (grams, kilograms, pounds, ounces)
//! - Length (meters, kilometers, feet, inches, miles)

use crate::{
    parser::units::duration_to_seconds, ConversionTarget, DurationUnit, LemmaError, LemmaResult,
    LengthUnit, LiteralValue, MassUnit, NumericUnit, PowerUnit, TemperatureUnit,
};
use rust_decimal::Decimal;

/// Convert a unit to a target unit (used internally by arithmetic operations)
pub(crate) fn convert_unit_for_arithmetic(
    value: &LiteralValue,
    target: &ConversionTarget,
) -> LemmaResult<LiteralValue> {
    let unit = match value {
        LiteralValue::Unit(u) => u,
        _ => {
            return Err(LemmaError::Engine(
                "Cannot convert non-unit value for arithmetic".to_string(),
            ))
        }
    };

    let converted_value = match (unit, target) {
        (NumericUnit::Duration(v, from), ConversionTarget::Duration(to)) => {
            convert_duration(*v, from, to)?
        }
        (NumericUnit::Mass(v, from), ConversionTarget::Mass(to)) => convert_mass(*v, from, to)?,
        (NumericUnit::Length(v, from), ConversionTarget::Length(to)) => {
            convert_length(*v, from, to)?
        }
        (NumericUnit::Temperature(v, from), ConversionTarget::Temperature(to)) => {
            convert_temperature(*v, from, to)?
        }
        (NumericUnit::Power(v, from), ConversionTarget::Power(to)) => convert_power(*v, from, to)?,
        (NumericUnit::Volume(v, from), ConversionTarget::Volume(to)) => {
            convert_volume(*v, from, to)?
        }
        (NumericUnit::Force(v, from), ConversionTarget::Force(to)) => convert_force(*v, from, to)?,
        (NumericUnit::Pressure(v, from), ConversionTarget::Pressure(to)) => {
            convert_pressure(*v, from, to)?
        }
        (NumericUnit::Energy(v, from), ConversionTarget::Energy(to)) => {
            convert_energy(*v, from, to)?
        }
        (NumericUnit::Frequency(v, from), ConversionTarget::Frequency(to)) => {
            convert_frequency(*v, from, to)?
        }
        (NumericUnit::DataSize(v, from), ConversionTarget::DataSize(to)) => {
            convert_data_size(*v, from, to)?
        }
        (NumericUnit::Money(v, from), ConversionTarget::Money(to)) => {
            if from == to {
                *v
            } else {
                return Err(LemmaError::Engine(format!(
                    "Cannot convert between different currencies: {:?} to {:?}",
                    from, to
                )));
            }
        }
        _ => {
            return Err(LemmaError::Engine(
                "Mismatched unit type for conversion".to_string(),
            ))
        }
    };

    let result_unit = match target {
        ConversionTarget::Mass(u) => NumericUnit::Mass(converted_value, u.clone()),
        ConversionTarget::Length(u) => NumericUnit::Length(converted_value, u.clone()),
        ConversionTarget::Volume(u) => NumericUnit::Volume(converted_value, u.clone()),
        ConversionTarget::Duration(u) => NumericUnit::Duration(converted_value, u.clone()),
        ConversionTarget::Temperature(u) => NumericUnit::Temperature(converted_value, u.clone()),
        ConversionTarget::Power(u) => NumericUnit::Power(converted_value, u.clone()),
        ConversionTarget::Force(u) => NumericUnit::Force(converted_value, u.clone()),
        ConversionTarget::Pressure(u) => NumericUnit::Pressure(converted_value, u.clone()),
        ConversionTarget::Energy(u) => NumericUnit::Energy(converted_value, u.clone()),
        ConversionTarget::Frequency(u) => NumericUnit::Frequency(converted_value, u.clone()),
        ConversionTarget::DataSize(u) => NumericUnit::DataSize(converted_value, u.clone()),
        ConversionTarget::Money(u) => NumericUnit::Money(converted_value, u.clone()),
        ConversionTarget::Percentage => {
            return Err(LemmaError::Engine(
                "Cannot convert to percentage for arithmetic".to_string(),
            ));
        }
    };

    Ok(LiteralValue::Unit(result_unit))
}

/// Convert a value to a target unit (for `in` operator)
/// - Unit → Number: extracts numeric value in target unit
/// - Number → Unit: creates a unit with that value
pub fn convert_unit(value: &LiteralValue, target: &ConversionTarget) -> LemmaResult<LiteralValue> {
    match value {
        LiteralValue::Unit(unit) => {
            let converted_value = match (unit, target) {
                (NumericUnit::Duration(v, from), ConversionTarget::Duration(to)) => {
                    convert_duration(*v, from, to)?
                }
                (NumericUnit::Mass(v, from), ConversionTarget::Mass(to)) => {
                    convert_mass(*v, from, to)?
                }
                (NumericUnit::Length(v, from), ConversionTarget::Length(to)) => {
                    convert_length(*v, from, to)?
                }
                (NumericUnit::Temperature(v, from), ConversionTarget::Temperature(to)) => {
                    convert_temperature(*v, from, to)?
                }
                (NumericUnit::Power(v, from), ConversionTarget::Power(to)) => {
                    convert_power(*v, from, to)?
                }
                (NumericUnit::Volume(v, from), ConversionTarget::Volume(to)) => {
                    convert_volume(*v, from, to)?
                }
                (NumericUnit::Force(v, from), ConversionTarget::Force(to)) => {
                    convert_force(*v, from, to)?
                }
                (NumericUnit::Pressure(v, from), ConversionTarget::Pressure(to)) => {
                    convert_pressure(*v, from, to)?
                }
                (NumericUnit::Energy(v, from), ConversionTarget::Energy(to)) => {
                    convert_energy(*v, from, to)?
                }
                (NumericUnit::Frequency(v, from), ConversionTarget::Frequency(to)) => {
                    convert_frequency(*v, from, to)?
                }
                (NumericUnit::DataSize(v, from), ConversionTarget::DataSize(to)) => {
                    convert_data_size(*v, from, to)?
                }
                (NumericUnit::Money(v, from), ConversionTarget::Money(to)) => {
                    if from == to {
                        *v
                    } else {
                        return Err(LemmaError::Engine(format!(
                            "Cannot convert between different currencies: {:?} to {:?}",
                            from, to
                        )));
                    }
                }
                _ => {
                    return Err(LemmaError::Engine(
                        "Mismatched unit type for conversion".to_string(),
                    ))
                }
            };
            Ok(LiteralValue::Number(converted_value))
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
                ConversionTarget::DataSize(u) => {
                    LiteralValue::Unit(NumericUnit::DataSize(*n, u.clone()))
                }
                ConversionTarget::Money(u) => LiteralValue::Unit(NumericUnit::Money(*n, u.clone())),
                ConversionTarget::Percentage => LiteralValue::Percentage(n * Decimal::from(100)),
            };
            Ok(result)
        }

        _ => Err(LemmaError::Engine(
            "Cannot convert value to target".to_string(),
        )),
    }
}

/// Convert seconds to a target duration unit (time-based only)
fn seconds_to_unit(seconds: Decimal, to: &DurationUnit) -> LemmaResult<Decimal> {
    let result = match to {
        DurationUnit::Second => seconds,
        DurationUnit::Minute => seconds / Decimal::from(60),
        DurationUnit::Hour => seconds / Decimal::from(3600),
        DurationUnit::Day => seconds / Decimal::from(86400),
        DurationUnit::Week => seconds / Decimal::from(604800),
        DurationUnit::Millisecond => seconds * Decimal::from(1000),
        DurationUnit::Microsecond => seconds * Decimal::from(1000000),
        // Calendar units should never reach here
        DurationUnit::Month | DurationUnit::Year => {
            unreachable!("Calendar units should be rejected in convert_duration")
        }
    };

    Ok(result)
}

/// Convert mass between different units
pub(crate) fn convert_mass(value: Decimal, from: &MassUnit, to: &MassUnit) -> LemmaResult<Decimal> {
    if from == to {
        return Ok(value);
    }

    let grams = match from {
        MassUnit::Gram => value,
        MassUnit::Milligram => value / Decimal::from(1000),
        MassUnit::Kilogram => value * Decimal::from(1000),
        MassUnit::Ton => value * Decimal::from(1000000),
        MassUnit::Pound => value * Decimal::new(45359237, 5), // 453.59237 grams
        MassUnit::Ounce => value * Decimal::new(2834952, 5),  // 28.34952 grams
    };

    let result = match to {
        MassUnit::Gram => grams,
        MassUnit::Milligram => grams * Decimal::from(1000),
        MassUnit::Kilogram => grams / Decimal::from(1000),
        MassUnit::Ton => grams / Decimal::from(1000000),
        MassUnit::Pound => grams / Decimal::new(45359237, 5),
        MassUnit::Ounce => grams / Decimal::new(2834952, 5),
    };

    Ok(result)
}

/// Convert length between different units
pub(crate) fn convert_length(
    value: Decimal,
    from: &LengthUnit,
    to: &LengthUnit,
) -> LemmaResult<Decimal> {
    if from == to {
        return Ok(value);
    }

    let meters = match from {
        LengthUnit::Meter => value,
        LengthUnit::Kilometer => value * Decimal::from(1000),
        LengthUnit::Decimeter => value / Decimal::from(10),
        LengthUnit::Centimeter => value / Decimal::from(100),
        LengthUnit::Millimeter => value / Decimal::from(1000),
        LengthUnit::Foot => value * Decimal::new(3048, 4), // 0.3048 meters
        LengthUnit::Inch => value * Decimal::new(254, 4),  // 0.0254 meters
        LengthUnit::Yard => value * Decimal::new(9144, 4), // 0.9144 meters
        LengthUnit::Mile => value * Decimal::new(1609344, 3), // 1609.344 meters
        LengthUnit::NauticalMile => value * Decimal::from(1852), // 1852 meters
    };

    let result = match to {
        LengthUnit::Meter => meters,
        LengthUnit::Kilometer => meters / Decimal::from(1000),
        LengthUnit::Decimeter => meters * Decimal::from(10),
        LengthUnit::Centimeter => meters * Decimal::from(100),
        LengthUnit::Millimeter => meters * Decimal::from(1000),
        LengthUnit::Foot => meters / Decimal::new(3048, 4),
        LengthUnit::Inch => meters / Decimal::new(254, 4),
        LengthUnit::Yard => meters / Decimal::new(9144, 4),
        LengthUnit::Mile => meters / Decimal::new(1609344, 3),
        LengthUnit::NauticalMile => meters / Decimal::from(1852),
    };

    Ok(result)
}

/// Convert duration between different units
pub(crate) fn convert_duration(
    value: Decimal,
    from: &DurationUnit,
    to: &DurationUnit,
) -> LemmaResult<Decimal> {
    if from == to {
        return Ok(value);
    }

    // Month and Year are calendar units, not time durations
    if matches!(from, DurationUnit::Month | DurationUnit::Year)
        || matches!(to, DurationUnit::Month | DurationUnit::Year)
    {
        return Err(LemmaError::Engine(
            "Cannot convert calendar units (month/year) to other duration units. Use date arithmetic instead.".to_string()
        ));
    }

    // Convert to base unit (seconds)
    let seconds = duration_to_seconds(value, from);

    // Convert from seconds to target unit
    let result = seconds_to_unit(seconds, to)?;

    Ok(result)
}

/// Convert temperature between different units
pub(crate) fn convert_temperature(
    value: Decimal,
    from: &TemperatureUnit,
    to: &TemperatureUnit,
) -> LemmaResult<Decimal> {
    if from == to {
        return Ok(value);
    }

    let celsius = match from {
        TemperatureUnit::Celsius => value,
        TemperatureUnit::Fahrenheit => {
            (value - Decimal::from(32)) * Decimal::new(5, 0) / Decimal::new(9, 0)
        }
        TemperatureUnit::Kelvin => value - Decimal::new(27315, 2), // 273.15
    };

    let result = match to {
        TemperatureUnit::Celsius => celsius,
        TemperatureUnit::Fahrenheit => {
            celsius * Decimal::new(9, 0) / Decimal::new(5, 0) + Decimal::from(32)
        }
        TemperatureUnit::Kelvin => celsius + Decimal::new(27315, 2), // 273.15
    };

    Ok(result)
}

/// Convert power between different units
pub(crate) fn convert_power(
    value: Decimal,
    from: &PowerUnit,
    to: &PowerUnit,
) -> LemmaResult<Decimal> {
    if from == to {
        return Ok(value);
    }

    let watts = match from {
        PowerUnit::Watt => value,
        PowerUnit::Kilowatt => value * Decimal::from(1000),
        PowerUnit::Megawatt => value * Decimal::from(1000000),
        PowerUnit::Milliwatt => value / Decimal::from(1000),
        PowerUnit::Horsepower => value * Decimal::new(7457, 1), // 745.7 watts
    };

    let result = match to {
        PowerUnit::Watt => watts,
        PowerUnit::Kilowatt => watts / Decimal::from(1000),
        PowerUnit::Megawatt => watts / Decimal::from(1000000),
        PowerUnit::Milliwatt => watts * Decimal::from(1000),
        PowerUnit::Horsepower => watts / Decimal::new(7457, 1),
    };

    Ok(result)
}

/// Convert volume between different units
pub(crate) fn convert_volume(
    value: Decimal,
    from: &crate::VolumeUnit,
    to: &crate::VolumeUnit,
) -> LemmaResult<Decimal> {
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
        Gallon => value * Decimal::new(3785411784, 9), // 3.785411784 liters (exact)
        Quart => value * Decimal::new(946352946, 9),   // 0.946352946 liters (1/4 gallon)
        Pint => value * Decimal::new(473176473, 9),    // 0.473176473 liters (1/2 quart)
        FluidOunce => value * Decimal::new(2957352956, 11), // 0.02957352956 liters (1/16 pint)
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

/// Convert force between different units
pub(crate) fn convert_force(
    value: Decimal,
    from: &crate::ForceUnit,
    to: &crate::ForceUnit,
) -> LemmaResult<Decimal> {
    use crate::ForceUnit::*;
    if from == to {
        return Ok(value);
    }

    let newtons = match from {
        Newton => value,
        Kilonewton => value * Decimal::from(1000),
        Lbf => value * Decimal::new(44482, 5), // 4.44822
    };

    let result = match to {
        Newton => newtons,
        Kilonewton => newtons / Decimal::from(1000),
        Lbf => newtons / Decimal::new(44482, 5),
    };

    Ok(result)
}

/// Convert pressure between different units
pub(crate) fn convert_pressure(
    value: Decimal,
    from: &crate::PressureUnit,
    to: &crate::PressureUnit,
) -> LemmaResult<Decimal> {
    use crate::PressureUnit::*;
    if from == to {
        return Ok(value);
    }

    let pascals = match from {
        Pascal => value,
        Kilopascal => value * Decimal::from(1000),
        Megapascal => value * Decimal::from(1000000),
        Bar => value * Decimal::from(100000),
        Atmosphere => value * Decimal::new(101325, 0), // 101325
        Psi => value * Decimal::new(689476, 2),        // 6894.76
        Torr => value * Decimal::new(13332237, 5),     // 133.32237
        Mmhg => value * Decimal::new(13332237, 5),     // 133.32237 (same as Torr)
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

/// Convert energy between different units
pub(crate) fn convert_energy(
    value: Decimal,
    from: &crate::EnergyUnit,
    to: &crate::EnergyUnit,
) -> LemmaResult<Decimal> {
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
        Calorie => value * Decimal::new(4184, 3), // 4.184
        Kilocalorie => value * Decimal::new(4184, 0), // 4184
        Btu => value * Decimal::new(105506, 2),   // 1055.06
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

/// Convert frequency between different units
pub(crate) fn convert_frequency(
    value: Decimal,
    from: &crate::FrequencyUnit,
    to: &crate::FrequencyUnit,
) -> LemmaResult<Decimal> {
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

/// Convert data size between different units
pub(crate) fn convert_data_size(
    value: Decimal,
    from: &crate::DataSizeUnit,
    to: &crate::DataSizeUnit,
) -> LemmaResult<Decimal> {
    use crate::DataSizeUnit::*;
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
        Mebibyte => value * Decimal::from(1048576), // 1024^2
        Gibibyte => value * Decimal::from(1073741824i64), // 1024^3
        Tebibyte => value * Decimal::from(1099511627776i64), // 1024^4
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
