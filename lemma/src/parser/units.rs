//! Unit resolution - maps unit strings to Lemma unit types

use crate::error::LemmaError;
use crate::semantic::*;
use rust_decimal::Decimal;

/// Resolve a unit string and value to a LiteralValue
pub fn resolve_unit(value: Decimal, unit_str: &str) -> Result<LiteralValue, LemmaError> {
    let unit_lower = unit_str.to_lowercase();

    // Try each unit category in order and wrap in NumericUnit
    if let Some(unit) = try_parse_mass_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Mass(value, unit)));
    }

    if let Some(unit) = try_parse_length_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Length(value, unit)));
    }

    if let Some(unit) = try_parse_volume_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Volume(value, unit)));
    }

    if let Some(unit) = try_parse_duration_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Duration(value, unit)));
    }

    if let Some(unit) = try_parse_temperature_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Temperature(value, unit)));
    }

    if let Some(unit) = try_parse_power_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Power(value, unit)));
    }

    if let Some(unit) = try_parse_force_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Force(value, unit)));
    }

    if let Some(unit) = try_parse_pressure_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Pressure(value, unit)));
    }

    if let Some(unit) = try_parse_energy_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Energy(value, unit)));
    }

    if let Some(unit) = try_parse_frequency_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Frequency(value, unit)));
    }

    if let Some(unit) = try_parse_data_size_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::DataSize(value, unit)));
    }

    if let Some(currency) = try_parse_money_unit(&unit_lower) {
        return Ok(LiteralValue::Unit(NumericUnit::Money(value, currency)));
    }

    // Unit not recognized
    let suggestion = find_closest_unit(&unit_lower);
    Err(LemmaError::Engine(format!(
        "Unknown unit: '{}'. {}",
        unit_str, suggestion
    )))
}

// Mass Units
fn try_parse_mass_unit(s: &str) -> Option<MassUnit> {
    match s {
        "kilogram" | "kilograms" => Some(MassUnit::Kilogram),
        "gram" | "grams" => Some(MassUnit::Gram),
        "milligram" | "milligrams" => Some(MassUnit::Milligram),
        "ton" | "tons" | "tonne" | "tonnes" => Some(MassUnit::Ton),
        "pound" | "pounds" => Some(MassUnit::Pound),
        "ounce" | "ounces" => Some(MassUnit::Ounce),
        _ => None,
    }
}

// Length Units
fn try_parse_length_unit(s: &str) -> Option<LengthUnit> {
    match s {
        "kilometer" | "kilometers" | "kilometre" | "kilometres" => Some(LengthUnit::Kilometer),
        "mile" | "miles" => Some(LengthUnit::Mile),
        "nautical_mile" | "nautical_miles" | "nauticalmile" | "nauticalmiles" => {
            Some(LengthUnit::NauticalMile)
        }
        "meter" | "meters" | "metre" | "metres" => Some(LengthUnit::Meter),
        "decimeter" | "decimeters" | "decimetre" | "decimetres" => Some(LengthUnit::Decimeter),
        "centimeter" | "centimeters" | "centimetre" | "centimetres" => Some(LengthUnit::Centimeter),
        "millimeter" | "millimeters" | "millimetre" | "millimetres" => Some(LengthUnit::Millimeter),
        "yard" | "yards" => Some(LengthUnit::Yard),
        "foot" | "feet" => Some(LengthUnit::Foot),
        "inch" | "inches" => Some(LengthUnit::Inch),
        _ => None,
    }
}

// Volume Units
fn try_parse_volume_unit(s: &str) -> Option<VolumeUnit> {
    match s {
        "cubic_meter" | "cubic_meters" | "cubic_metre" | "cubic_metres" | "cubicmeter"
        | "cubicmeters" | "cubicmetre" | "cubicmetres" => Some(VolumeUnit::CubicMeter),
        "cubic_centimeter" | "cubic_centimeters" | "cubic_centimetre" | "cubic_centimetres"
        | "cubiccentimeter" | "cubiccentimeters" => Some(VolumeUnit::CubicCentimeter),
        "liter" | "liters" | "litre" | "litres" => Some(VolumeUnit::Liter),
        "deciliter" | "deciliters" | "decilitre" | "decilitres" => Some(VolumeUnit::Deciliter),
        "centiliter" | "centiliters" | "centilitre" | "centilitres" => Some(VolumeUnit::Centiliter),
        "milliliter" | "milliliters" | "millilitre" | "millilitres" => Some(VolumeUnit::Milliliter),
        "gallon" | "gallons" => Some(VolumeUnit::Gallon),
        "quart" | "quarts" => Some(VolumeUnit::Quart),
        "pint" | "pints" => Some(VolumeUnit::Pint),
        "fluid_ounce" | "fluid_ounces" | "fluidounce" | "fluidounces" => {
            Some(VolumeUnit::FluidOunce)
        }
        _ => None,
    }
}

// Duration Units
fn try_parse_duration_unit(s: &str) -> Option<DurationUnit> {
    match s {
        "year" | "years" => Some(DurationUnit::Year),
        "month" | "months" => Some(DurationUnit::Month),
        "week" | "weeks" => Some(DurationUnit::Week),
        "day" | "days" => Some(DurationUnit::Day),
        "hour" | "hours" => Some(DurationUnit::Hour),
        "minute" | "minutes" => Some(DurationUnit::Minute),
        "second" | "seconds" => Some(DurationUnit::Second),
        "millisecond" | "milliseconds" => Some(DurationUnit::Millisecond),
        "microsecond" | "microseconds" => Some(DurationUnit::Microsecond),
        _ => None,
    }
}

// Duration conversion constants (all relative to seconds)
const SECONDS_PER_MINUTE: i32 = 60;
const SECONDS_PER_HOUR: i32 = 3600;              // 60 * 60
const SECONDS_PER_DAY: i32 = 86400;              // 24 * 60 * 60
const SECONDS_PER_WEEK: i32 = 604800;            // 7 * 24 * 60 * 60
const MILLISECONDS_PER_SECOND: i32 = 1000;
const MICROSECONDS_PER_SECOND: i32 = 1000000;

/// Convert a time-based duration value to seconds.
///
/// Note: Month and Year are calendar units that depend on specific dates
/// (e.g., February has 28 or 29 days), so they cannot be converted to fixed
/// second values and must be handled separately using chrono's date arithmetic.
pub(crate) fn duration_to_seconds(value: Decimal, unit: &DurationUnit) -> Decimal {
    match unit {
        DurationUnit::Microsecond => value / Decimal::from(MICROSECONDS_PER_SECOND),
        DurationUnit::Millisecond => value / Decimal::from(MILLISECONDS_PER_SECOND),
        DurationUnit::Second => value,
        DurationUnit::Minute => value * Decimal::from(SECONDS_PER_MINUTE),
        DurationUnit::Hour => value * Decimal::from(SECONDS_PER_HOUR),
        DurationUnit::Day => value * Decimal::from(SECONDS_PER_DAY),
        DurationUnit::Week => value * Decimal::from(SECONDS_PER_WEEK),
        DurationUnit::Month | DurationUnit::Year => {
            unreachable!("Calendar units (month/year) should be handled by date arithmetic")
        }
    }
}

fn try_parse_temperature_unit(s: &str) -> Option<TemperatureUnit> {
    match s {
        "celsius" => Some(TemperatureUnit::Celsius),
        "fahrenheit" => Some(TemperatureUnit::Fahrenheit),
        "kelvin" => Some(TemperatureUnit::Kelvin),
        _ => None,
    }
}

// Power Units
fn try_parse_power_unit(s: &str) -> Option<PowerUnit> {
    match s {
        "megawatt" | "megawatts" => Some(PowerUnit::Megawatt),
        "kilowatt" | "kilowatts" => Some(PowerUnit::Kilowatt),
        "watt" | "watts" => Some(PowerUnit::Watt),
        "milliwatt" | "milliwatts" => Some(PowerUnit::Milliwatt),
        "horsepower" => Some(PowerUnit::Horsepower),
        _ => None,
    }
}

// Force Units
fn try_parse_force_unit(s: &str) -> Option<ForceUnit> {
    match s {
        "newton" | "newtons" => Some(ForceUnit::Newton),
        "kilonewton" | "kilonewtons" => Some(ForceUnit::Kilonewton),
        "lbf" | "poundforce" => Some(ForceUnit::Lbf),
        _ => None,
    }
}

// Pressure Units
fn try_parse_pressure_unit(s: &str) -> Option<PressureUnit> {
    match s {
        "megapascal" | "megapascals" => Some(PressureUnit::Megapascal),
        "kilopascal" | "kilopascals" => Some(PressureUnit::Kilopascal),
        "pascal" | "pascals" => Some(PressureUnit::Pascal),
        "atmosphere" | "atmospheres" => Some(PressureUnit::Atmosphere),
        "bar" => Some(PressureUnit::Bar),
        "psi" => Some(PressureUnit::Psi),
        "torr" => Some(PressureUnit::Torr),
        "mmhg" => Some(PressureUnit::Mmhg),
        _ => None,
    }
}

// Energy Units
fn try_parse_energy_unit(s: &str) -> Option<EnergyUnit> {
    match s {
        "megajoule" | "megajoules" => Some(EnergyUnit::Megajoule),
        "kilojoule" | "kilojoules" => Some(EnergyUnit::Kilojoule),
        "joule" | "joules" => Some(EnergyUnit::Joule),
        "kilowatthour" | "kilowatthours" => Some(EnergyUnit::Kilowatthour),
        "watthour" | "watthours" => Some(EnergyUnit::Watthour),
        "kilocalorie" | "kilocalories" => Some(EnergyUnit::Kilocalorie),
        "calorie" | "calories" => Some(EnergyUnit::Calorie),
        "btu" => Some(EnergyUnit::Btu),
        _ => None,
    }
}

// Frequency Units
fn try_parse_frequency_unit(s: &str) -> Option<FrequencyUnit> {
    match s {
        "hertz" => Some(FrequencyUnit::Hertz),
        "kilohertz" => Some(FrequencyUnit::Kilohertz),
        "megahertz" => Some(FrequencyUnit::Megahertz),
        "gigahertz" => Some(FrequencyUnit::Gigahertz),
        _ => None,
    }
}

// Data Size Units
fn try_parse_data_size_unit(s: &str) -> Option<DataSizeUnit> {
    match s {
        "petabyte" | "petabytes" => Some(DataSizeUnit::Petabyte),
        "terabyte" | "terabytes" => Some(DataSizeUnit::Terabyte),
        "gigabyte" | "gigabytes" => Some(DataSizeUnit::Gigabyte),
        "megabyte" | "megabytes" => Some(DataSizeUnit::Megabyte),
        "kilobyte" | "kilobytes" => Some(DataSizeUnit::Kilobyte),
        "byte" | "bytes" => Some(DataSizeUnit::Byte),
        "tebibyte" | "tebibytes" => Some(DataSizeUnit::Tebibyte),
        "gibibyte" | "gibibytes" => Some(DataSizeUnit::Gibibyte),
        "mebibyte" | "mebibytes" => Some(DataSizeUnit::Mebibyte),
        "kibibyte" | "kibibytes" => Some(DataSizeUnit::Kibibyte),
        _ => None,
    }
}

// Money Units (ISO 4217 3-character currency codes only)
fn try_parse_money_unit(s: &str) -> Option<MoneyUnit> {
    match s {
        "eur" => Some(MoneyUnit::Eur),
        "usd" => Some(MoneyUnit::Usd),
        "gbp" => Some(MoneyUnit::Gbp),
        "jpy" => Some(MoneyUnit::Jpy),
        "cny" => Some(MoneyUnit::Cny),
        "chf" => Some(MoneyUnit::Chf),
        "cad" => Some(MoneyUnit::Cad),
        "aud" => Some(MoneyUnit::Aud),
        "inr" => Some(MoneyUnit::Inr),
        _ => None,
    }
}

/// Resolve a unit conversion target (for "in" expressions)
pub fn resolve_conversion_target(unit_str: &str) -> Result<ConversionTarget, LemmaError> {
    let unit_lower = unit_str.to_lowercase();

    // Handle "percentage" conversion
    if unit_lower == "percentage" || unit_lower == "percent" {
        return Ok(ConversionTarget::Percentage);
    }

    // Try each unit category
    if let Some(unit) = try_parse_mass_unit(&unit_lower) {
        return Ok(ConversionTarget::Mass(unit));
    }

    if let Some(unit) = try_parse_length_unit(&unit_lower) {
        return Ok(ConversionTarget::Length(unit));
    }

    if let Some(unit) = try_parse_volume_unit(&unit_lower) {
        return Ok(ConversionTarget::Volume(unit));
    }

    if let Some(unit) = try_parse_duration_unit(&unit_lower) {
        return Ok(ConversionTarget::Duration(unit));
    }

    if let Some(unit) = try_parse_temperature_unit(&unit_lower) {
        return Ok(ConversionTarget::Temperature(unit));
    }

    if let Some(unit) = try_parse_power_unit(&unit_lower) {
        return Ok(ConversionTarget::Power(unit));
    }

    if let Some(unit) = try_parse_force_unit(&unit_lower) {
        return Ok(ConversionTarget::Force(unit));
    }

    if let Some(unit) = try_parse_pressure_unit(&unit_lower) {
        return Ok(ConversionTarget::Pressure(unit));
    }

    if let Some(unit) = try_parse_energy_unit(&unit_lower) {
        return Ok(ConversionTarget::Energy(unit));
    }

    if let Some(unit) = try_parse_frequency_unit(&unit_lower) {
        return Ok(ConversionTarget::Frequency(unit));
    }

    if let Some(unit) = try_parse_data_size_unit(&unit_lower) {
        return Ok(ConversionTarget::DataSize(unit));
    }

    if let Some(unit) = try_parse_money_unit(&unit_lower) {
        return Ok(ConversionTarget::Money(unit));
    }

    // Conversion target not recognized
    let suggestion = find_closest_unit(&unit_lower);
    Err(LemmaError::Engine(format!(
        "Unknown conversion target unit: '{}'. {}",
        unit_str, suggestion
    )))
}

/// Find the closest matching unit to provide a helpful suggestion
fn find_closest_unit(s: &str) -> String {
    // Common typos and alternatives
    let suggestions: Vec<(&str, &str)> = vec![
        ("kilometer", "kilometers"),
        ("kilometre", "kilometers"),
        ("metre", "meters"),
        ("kilogramme", "kilograms"),
        ("gramme", "grams"),
        ("litre", "liters"),
        ("sec", "seconds"),
        ("min", "minutes"),
        ("hr", "hours"),
    ];

    for (typo, correct) in &suggestions {
        if s == *typo || s.starts_with(typo) {
            return format!("Did you mean '{}'?", correct);
        }
    }

    // Check for common abbreviation issues
    if s.len() <= 3 {
        return "Try using the full unit name (e.g., 'kilometers' instead of 'km')".to_string();
    }

    "Check the unit name spelling".to_string()
}
