//! Port of `benchmarks/java` `SpecGenerator.logistics`: multi-spec logistics
//! rating workspace on the research ladder 1050 / 6300 / 18900 / 126000 rate cells.
//!
//! The Rust and Java generators must emit byte-identical source so both
//! benches measure the same bytes; [`LADDER`] carries the expected length of
//! each profile and [`logistics`] asserts it.

use lemma::ResourceLimits;
use std::collections::HashMap;
use std::fmt::Write;

/// UPS Ground zones 2 to 8.
pub const ZONES: usize = 7;
/// 1 to 150 lb breaks.
pub const WEIGHTS: usize = 150;
/// Cells in one Ground matrix.
pub const GROUND_CELLS: usize = ZONES * WEIGHTS;
/// Services on the UPS zone chart header.
pub const SERVICES: usize = 6;
/// US ZIP3 prefixes.
pub const ZIP3_COUNT: usize = 900;
/// D2C multi-carrier count.
pub const D2C_CARRIERS: usize = 3;
/// Enterprise low contract count.
pub const ENTERPRISE_CONTRACTS: usize = 20;

const SERVICE_IDS: [&str; SERVICES] = [
    "ground",
    "select_3day",
    "air_2day",
    "air_2day_am",
    "nda_saver",
    "nda",
];

/// One rung of the research ladder with the source length the Java generator
/// produces for it.
pub struct Rung {
    pub rate_cells: usize,
    pub source_bytes: usize,
}

pub const LADDER: [Rung; 4] = [
    Rung {
        rate_cells: GROUND_CELLS,
        source_bytes: 91_193,
    },
    Rung {
        rate_cells: GROUND_CELLS * SERVICES,
        source_bytes: 531_593,
    },
    Rung {
        rate_cells: GROUND_CELLS * SERVICES * D2C_CARRIERS,
        source_bytes: 1_589_057,
    },
    Rung {
        rate_cells: GROUND_CELLS * SERVICES * ENTERPRISE_CONTRACTS,
        source_bytes: 10_581_400,
    },
];

pub struct LogisticsFixture {
    pub profile: &'static str,
    pub rate_cells: usize,
    pub source: String,
    pub limits: ResourceLimits,
}

impl LogisticsFixture {
    pub const SPEC_NAME: &'static str = "rate_shop";
    pub const TERMINAL_RULE: &'static str = "cheapest";

    /// Shipment inputs, same values as the Java `shipmentData()`.
    pub fn inputs() -> HashMap<String, String> {
        HashMap::from([
            ("length_in".into(), "12".into()),
            ("width_in".into(), "10".into()),
            ("height_in".into(), "8".into()),
            ("actual_lb".into(), "5".into()),
            ("dest_zip3".into(), "100".into()),
            ("is_residential".into(), "true".into()),
            ("is_das".into(), "false".into()),
            ("diesel_usd".into(), "4.50".into()),
            ("jet_usd".into(), "2.40".into()),
            ("discount_pct".into(), "10".into()),
        ])
    }
}

struct Profile {
    name: &'static str,
    carriers: Vec<String>,
    service_count: usize,
    rate_cells: usize,
}

fn profile(rate_cells: usize) -> Profile {
    match rate_cells {
        GROUND_CELLS => Profile {
            name: "ground",
            carriers: vec!["ups".to_string()],
            service_count: 1,
            rate_cells,
        },
        cells if cells == GROUND_CELLS * SERVICES => Profile {
            name: "carrier",
            carriers: vec!["ups".to_string()],
            service_count: SERVICES,
            rate_cells,
        },
        cells if cells == GROUND_CELLS * SERVICES * D2C_CARRIERS => Profile {
            name: "d2c",
            carriers: vec![
                "ups".to_string(),
                "fedex".to_string(),
                "regional".to_string(),
            ],
            service_count: SERVICES,
            rate_cells,
        },
        cells if cells == GROUND_CELLS * SERVICES * ENTERPRISE_CONTRACTS => Profile {
            name: "enterprise",
            carriers: (1..=ENTERPRISE_CONTRACTS)
                .map(|index| format!("contract_{index:02}"))
                .collect(),
            service_count: SERVICES,
            rate_cells,
        },
        other => panic!(
            "BUG: logistics rate_cells must be one of {:?}, got {other}",
            LADDER.map(|rung| rung.rate_cells)
        ),
    }
}

/// Mirrors the Java `BenchLimits.forSize(limitScale)` overrides.
fn limits_for_scale(scale: usize) -> ResourceLimits {
    let scale = scale.max(1_000);
    let source_bytes = (16 * 1024 * 1024).max(scale * 256);
    ResourceLimits {
        max_source_size_bytes: source_bytes,
        max_expression_count: 65_536.max(scale * 64),
        max_normalized_expression_nodes: 100_000.max(scale * 128),
        max_loaded_bytes: (100 * 1024 * 1024).max(source_bytes * 4),
        ..ResourceLimits::default()
    }
}

/// Java `%.2f`. Every emitted price is an exact two-decimal value in real
/// arithmetic (sums of two-decimal terms), so the double sits within 1e-13 of
/// it and both Java's half-up-on-shortest-digits and Rust's exact-binary
/// rounding land on the same two decimals; the byte-length assertion guards it.
fn two_decimals(value: f64) -> String {
    format!("{value:.2}")
}

pub fn logistics(rate_cells: usize) -> LogisticsFixture {
    let profile = profile(rate_cells);
    let rung = LADDER
        .iter()
        .find(|rung| rung.rate_cells == rate_cells)
        .expect("BUG: profile() accepted a rate_cells value absent from LADDER");
    let mut out = String::with_capacity(rung.source_bytes);

    emit_accessorials(&mut out);
    out.push('\n');
    emit_fuel_ground(&mut out);
    out.push('\n');
    emit_fuel_air(&mut out);
    out.push('\n');

    let mut quote_spec_names = Vec::new();
    for carrier in &profile.carriers {
        emit_zones_spec(&mut out, carrier, profile.service_count);
        out.push('\n');
        for (service_index, service) in SERVICE_IDS
            .iter()
            .copied()
            .enumerate()
            .take(profile.service_count)
        {
            emit_rates_spec(&mut out, carrier, service, service_index);
            out.push('\n');
            let quote_name = format!("quote_{carrier}_{service}");
            emit_quote_pipeline(&mut out, &quote_name, carrier, service, service_index);
            out.push('\n');
            quote_spec_names.push(quote_name);
        }
    }

    emit_rate_shop(&mut out, &quote_spec_names);
    out.push('\n');

    assert_eq!(
        out.len(),
        rung.source_bytes,
        "logistics {} source diverged from the Java generator",
        profile.name
    );

    let limit_scale =
        profile.rate_cells + profile.carriers.len() * ZIP3_COUNT * profile.service_count + 256;

    LogisticsFixture {
        profile: profile.name,
        rate_cells: profile.rate_cells,
        source: out,
        limits: limits_for_scale(limit_scale),
    }
}

fn emit_accessorials(out: &mut String) {
    out.push_str("spec accessorials\n");
    out.push_str("\"\"\"Parcel accessorial schedule (tens of fees, not thousands).\"\"\"\n\n");
    out.push_str("data is_residential: boolean\n");
    out.push_str("data is_das: boolean\n");
    out.push_str("data billable_lb: number\n");
    out.push_str("data longest_side_in: number\n\n");
    out.push_str("rule residential_fee: 0\n");
    out.push_str("  unless is_residential then 6.50\n\n");
    out.push_str("rule das_fee: 0\n");
    out.push_str("  unless is_das and is_residential then 4.20\n");
    out.push_str("  unless is_das and not is_residential then 2.80\n\n");
    out.push_str("rule ahs_weight_fee: 0\n");
    out.push_str("  unless billable_lb > 50 then 30\n\n");
    out.push_str("rule ahs_length_fee: 0\n");
    out.push_str("  unless longest_side_in > 48 then 30\n\n");
    out.push_str("rule large_package_fee: 0\n");
    out.push_str("  unless longest_side_in >= 96 then 95\n\n");
    out.push_str("rule accessorial_total:\n");
    out.push_str(
        "  residential_fee + das_fee + ahs_weight_fee + ahs_length_fee + large_package_fee\n",
    );
}

fn emit_fuel_ground(out: &mut String) {
    out.push_str("spec fuel_ground\n");
    out.push_str("\"\"\"Ground fuel % from diesel index brackets (weekly table shape).\"\"\"\n\n");
    out.push_str("data diesel_usd: number\n\n");
    out.push_str("rule fuel_pct: 20%\n");
    let mut price: f64 = 2.50;
    let mut pct: f64 = 20.0;
    for _ in 0..24 {
        writeln!(
            out,
            "  unless diesel_usd >= {} then {}%",
            two_decimals(price),
            two_decimals(pct)
        )
        .expect("BUG: writing to String never fails");
        price += 0.27;
        pct += 0.25;
    }
}

fn emit_fuel_air(out: &mut String) {
    out.push_str("spec fuel_air\n");
    out.push_str("\"\"\"Air fuel % from jet index brackets.\"\"\"\n\n");
    out.push_str("data jet_usd: number\n\n");
    out.push_str("rule fuel_pct: 18%\n");
    let mut price: f64 = 1.50;
    let mut pct: f64 = 18.0;
    for _ in 0..24 {
        writeln!(
            out,
            "  unless jet_usd >= {} then {}%",
            two_decimals(price),
            two_decimals(pct)
        )
        .expect("BUG: writing to String never fails");
        price += 0.05;
        pct += 0.25;
    }
}

fn emit_zones_spec(out: &mut String, carrier: &str, service_count: usize) {
    writeln!(out, "spec zones_{carrier}").expect("BUG: writing to String never fails");
    out.push_str("\"\"\"ZIP3 → zone for ship-from origin (one warehouse chart).\"\"\"\n\n");
    out.push_str("data dest_zip3: text\n\n");
    for (service_index, service) in SERVICE_IDS.iter().copied().enumerate().take(service_count) {
        writeln!(out, "rule {service}_zone: 2").expect("BUG: writing to String never fails");
        for zip3 in 0..ZIP3_COUNT {
            let zone = 2 + (zip3 + service_index) % ZONES;
            writeln!(out, "  unless dest_zip3 is \"{zip3:03}\" then {zone}")
                .expect("BUG: writing to String never fails");
        }
        out.push('\n');
    }
}

fn emit_rates_spec(out: &mut String, carrier: &str, service: &str, service_index: usize) {
    writeln!(out, "spec rates_{carrier}_{service}").expect("BUG: writing to String never fails");
    writeln!(
        out,
        "\"\"\"Zone × lb rate card ({GROUND_CELLS} cells).\"\"\"\n"
    )
    .expect("BUG: writing to String never fails");
    out.push_str("data zone: number\n");
    out.push_str("data billable_lb: number\n\n");
    out.push_str("rule base_rate: 0\n");
    for zone_index in 0..ZONES {
        let zone = 2 + zone_index;
        for lb in 1..=WEIGHTS {
            let price = 8.0 + zone as f64 * 1.15 + lb as f64 * 0.42 + service_index as f64 * 0.75;
            writeln!(
                out,
                "  unless zone is {zone} and billable_lb >= {lb} then {}",
                two_decimals(price)
            )
            .expect("BUG: writing to String never fails");
        }
    }
}

fn emit_quote_pipeline(
    out: &mut String,
    quote_name: &str,
    carrier: &str,
    service: &str,
    service_index: usize,
) {
    let air = service_index >= 2;
    let fuel_spec = if air { "fuel_air" } else { "fuel_ground" };
    let fuel_data = if air { "jet_usd" } else { "diesel_usd" };

    writeln!(out, "spec {quote_name}").expect("BUG: writing to String never fails");
    writeln!(
        out,
        "\"\"\"Rating pipeline for {carrier} {service}.\"\"\"\n"
    )
    .expect("BUG: writing to String never fails");

    writeln!(out, "uses z: zones_{carrier}").expect("BUG: writing to String never fails");
    out.push_str("  -> with dest_zip3: dest_zip3\n\n");
    writeln!(out, "uses r: rates_{carrier}_{service}").expect("BUG: writing to String never fails");
    out.push_str("  -> with zone: zone\n");
    out.push_str("  -> with billable_lb: billable_lb\n\n");
    out.push_str("uses a: accessorials\n");
    out.push_str("  -> with is_residential: is_residential\n");
    out.push_str("  -> with is_das: is_das\n");
    out.push_str("  -> with billable_lb: billable_lb\n");
    out.push_str("  -> with longest_side_in: length_in\n\n");
    writeln!(out, "uses f: {fuel_spec}").expect("BUG: writing to String never fails");
    writeln!(out, "  -> with {fuel_data}: {fuel_data}\n")
        .expect("BUG: writing to String never fails");

    out.push_str("data length_in: number\n");
    out.push_str("data width_in: number\n");
    out.push_str("data height_in: number\n");
    out.push_str("data actual_lb: number\n");
    out.push_str("data dest_zip3: text\n");
    out.push_str("data is_residential: boolean\n");
    out.push_str("data is_das: boolean\n");
    out.push_str("data diesel_usd: number\n");
    out.push_str("data jet_usd: number\n");
    out.push_str("data discount_pct: number\n\n");

    out.push_str("rule dim_lb: (length_in * width_in * height_in) / 139\n");
    out.push_str("rule billable_lb: actual_lb\n");
    out.push_str("  unless dim_lb > actual_lb then dim_lb\n");
    writeln!(out, "rule zone: z.{service}_zone").expect("BUG: writing to String never fails");
    out.push_str("rule base: r.base_rate\n");
    out.push_str("rule accessorials_total: a.accessorial_total\n");
    out.push_str("rule pre_fuel: base + accessorials_total\n");
    out.push_str("rule fuel_amount: pre_fuel * f.fuel_pct\n");
    out.push_str("rule with_fuel: pre_fuel + fuel_amount\n");
    out.push_str("rule discount_amount: with_fuel * (discount_pct / 100)\n");
    out.push_str("rule with_discount: with_fuel - discount_amount\n");
    out.push_str("rule total: with_discount\n");
}

fn emit_rate_shop(out: &mut String, quote_spec_names: &[String]) {
    out.push_str("spec rate_shop\n");
    out.push_str("\"\"\"Wide rate-shop: compare carrier×service quotes, pick cheapest.\"\"\"\n\n");

    for (index, quote) in quote_spec_names.iter().enumerate() {
        writeln!(out, "uses q{index}: {quote}").expect("BUG: writing to String never fails");
        out.push_str("  -> with length_in: length_in\n");
        out.push_str("  -> with width_in: width_in\n");
        out.push_str("  -> with height_in: height_in\n");
        out.push_str("  -> with actual_lb: actual_lb\n");
        out.push_str("  -> with dest_zip3: dest_zip3\n");
        out.push_str("  -> with is_residential: is_residential\n");
        out.push_str("  -> with is_das: is_das\n");
        out.push_str("  -> with diesel_usd: diesel_usd\n");
        out.push_str("  -> with jet_usd: jet_usd\n");
        out.push_str("  -> with discount_pct: discount_pct\n\n");
    }

    out.push_str("data length_in: number\n");
    out.push_str("data width_in: number\n");
    out.push_str("data height_in: number\n");
    out.push_str("data actual_lb: number\n");
    out.push_str("data dest_zip3: text\n");
    out.push_str("data is_residential: boolean\n");
    out.push_str("data is_das: boolean\n");
    out.push_str("data diesel_usd: number\n");
    out.push_str("data jet_usd: number\n");
    out.push_str("data discount_pct: number\n\n");

    for index in 0..quote_spec_names.len() {
        writeln!(out, "rule quote_{index}: q{index}.total")
            .expect("BUG: writing to String never fails");
    }
    out.push('\n');
    if quote_spec_names.len() == 1 {
        out.push_str("rule cheapest: quote_0\n");
    } else {
        out.push_str("rule min_1: quote_0\n");
        out.push_str("  unless quote_1 < quote_0 then quote_1\n");
        for index in 2..quote_spec_names.len() {
            let previous = index - 1;
            writeln!(out, "rule min_{index}: min_{previous}")
                .expect("BUG: writing to String never fails");
            writeln!(
                out,
                "  unless quote_{index} < min_{previous} then quote_{index}"
            )
            .expect("BUG: writing to String never fails");
        }
        writeln!(out, "rule cheapest: min_{}", quote_spec_names.len() - 1)
            .expect("BUG: writing to String never fails");
    }
}
