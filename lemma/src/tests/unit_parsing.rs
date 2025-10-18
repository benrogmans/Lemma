use crate::parser::parse;

#[test]
fn test_in_expressions_comprehensive() {
    let test_cases = vec![
        ("100 in meters", "length conversion"),
        ("5 in kilograms", "mass conversion"),
        ("2.5 in liters", "volume conversion"),
        ("3600 in seconds", "time conversion"),
        ("25 in celsius", "temperature conversion"),
        ("1000 in watts", "power conversion"),
        ("50 in newtons", "force conversion"),
        ("101325 in pascals", "pressure conversion"),
        ("1000 in joules", "energy conversion"),
        ("440 in hertz", "frequency conversion"),
        ("1024 in bytes", "data size conversion"),
        ("100 in usd", "money conversion"),
        ("(100 + 50) in meters", "arithmetic with unit conversion"),
        ("salary in usd", "variable with unit conversion"),
        ("(age * 365) in days", "complex arithmetic with conversion"),
        ("0 in meters", "zero with unit"),
        ("1 in meters", "one with unit"),
        ("-5 in celsius", "negative with unit"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}

#[test]
fn test_all_unit_types_comprehensive() {
    let test_cases = vec![
        ("100 in liters", "liters"),
        ("50 in gallons", "gallons"),
        ("1000 in watts", "watts"),
        ("5 in kilowatts", "kilowatts"),
        ("2 in megawatts", "megawatts"),
        ("100 in horsepower", "horsepower"),
        ("50 in newtons", "newtons"),
        ("100 in kilonewtons", "kilonewtons"),
        ("75 in lbf", "pound-force"),
        ("101325 in pascals", "pascals"),
        ("100 in kilopascals", "kilopascals"),
        ("1 in megapascals", "megapascals"),
        ("1 in bar", "bar"),
        ("14.7 in psi", "psi"),
        ("1000 in joules", "joules"),
        ("5 in kilojoules", "kilojoules"),
        ("1 in megajoules", "megajoules"),
        ("1 in kilowatthour", "kilowatt-hour"),
        ("2000 in calorie", "calories"),
        ("500 in kilocalorie", "kilocalories"),
        ("440 in hertz", "hertz"),
        ("2.4 in gigahertz", "gigahertz"),
        ("100 in kilohertz", "kilohertz"),
        ("98.5 in megahertz", "megahertz"),
        ("1024 in bytes", "bytes"),
        ("1 in kilobytes", "kilobytes"),
        ("500 in megabytes", "megabytes"),
        ("100 in gigabytes", "gigabytes"),
        ("5 in terabytes", "terabytes"),
        ("100 in usd", "US dollars"),
        ("85 in eur", "euros"),
        ("75 in gbp", "British pounds"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}

#[test]
fn test_unit_literals_in_rules() {
    let test_cases = vec![
        ("5 kilograms", "kilograms"),
        ("100 grams", "grams"),
        ("500 milligrams", "milligrams"),
        ("5 tons", "tons"),
        ("10 pounds", "pounds"),
        ("8 ounces", "ounces"),
        ("100 meters", "meters"),
        ("5 kilometers", "kilometers"),
        ("10 miles", "miles"),
        ("50 nautical_miles", "nautical miles"),
        ("25 decimeters", "decimeters"),
        ("180 centimeters", "centimeters"),
        ("50 millimeters", "millimeters"),
        ("10 yards", "yards"),
        ("6 feet", "feet"),
        ("72 inches", "inches"),
        ("5 cubic_meters", "cubic meters"),
        ("1000 cubic_centimeters", "cubic centimeters"),
        ("2.5 liters", "liters"),
        ("5 deciliters", "deciliters"),
        ("10 centiliters", "centiliters"),
        ("500 milliliters", "milliliters"),
        ("1 gallon", "gallons"),
        ("2 quarts", "quarts"),
        ("4 pints", "pints"),
        ("16 fluid_ounces", "fluid ounces"),
        ("-5 celsius", "celsius"),
        ("98.6 fahrenheit", "fahrenheit"),
        ("273 kelvin", "kelvin"),
        ("2 years", "years"),
        ("6 months", "months"),
        ("52 weeks", "weeks"),
        ("365 days", "days"),
        ("24 hours", "hours"),
        ("60 minutes", "minutes"),
        ("3600 seconds", "seconds"),
        ("1000 milliseconds", "milliseconds"),
        ("500000 microseconds", "microseconds"),
        ("1000 watts", "watts"),
        ("500 milliwatts", "milliwatts"),
        ("5 kilowatts", "kilowatts"),
        ("2 megawatts", "megawatts"),
        ("100 horsepower", "horsepower"),
        ("1000 joules", "joules"),
        ("5 kilojoules", "kilojoules"),
        ("2 megajoules", "megajoules"),
        ("1 kilowatthour", "kilowatt-hour"),
        ("500 watthours", "watt-hours"),
        ("2000 calories", "calories"),
        ("100 kilocalories", "kilocalories"),
        ("5000 btu", "BTU"),
        ("50 newtons", "newtons"),
        ("100 kilonewtons", "kilonewtons"),
        ("101325 pascals", "pascals"),
        ("100 kilopascals", "kilopascals"),
        ("5 megapascals", "megapascals"),
        ("1 atmosphere", "atmosphere"),
        ("1 bar", "bar"),
        ("14.7 psi", "psi"),
        ("760 torr", "torr"),
        ("760 mmhg", "mmHg"),
        ("440 hertz", "hertz"),
        ("2.4 gigahertz", "gigahertz"),
        ("1024 bytes", "bytes"),
        ("10 kilobytes", "kilobytes"),
        ("500 megabytes", "megabytes"),
        ("100 gigabytes", "gigabytes"),
        ("5 terabytes", "terabytes"),
        ("1 petabyte", "petabyte"),
        ("1024 kibibytes", "kibibytes"),
        ("512 mebibytes", "mebibytes"),
        ("8 gibibytes", "gibibytes"),
        ("2 tebibytes", "tebibytes"),
        ("99.99 usd", "US dollars"),
        ("85.50 eur", "euros"),
        ("75 gbp", "British pounds"),
        ("10000 jpy", "Japanese yen"),
        ("500 cny", "Chinese yuan"),
        ("100 chf", "Swiss francs"),
        ("150 cad", "Canadian dollars"),
        ("200 aud", "Australian dollars"),
        ("5000 inr", "Indian rupees"),
        ("50 percent", "percent"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse unit literal {} ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}

#[test]
fn test_comparison_with_unit_conversions() {
    let test_cases = vec![
        (
            "(weight in kilograms) > 50",
            "unit conversion in comparison with parens",
        ),
        ("(height in meters) >= 1.8", "unit conversion with gte"),
        ("(distance in kilometers) < 100", "unit conversion with lt"),
        ("(temp in celsius) == 25", "unit conversion with equality"),
        (
            "(100 in meters) > (50 in feet)",
            "unit conversions on both sides",
        ),
        ("weight in kilograms > 50", "unit conversion without parens"),
        (
            "distance_km in miles > 50",
            "variable conversion in comparison",
        ),
        (
            "package_weight in pounds > weight_limit",
            "two variables with conversion",
        ),
        (
            "(x + 10 kilograms) in pounds > 50",
            "arithmetic with conversion in comparison",
        ),
        (
            "temp in fahrenheit >= 70 and temp in fahrenheit <= 90",
            "multiple comparisons",
        ),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}
