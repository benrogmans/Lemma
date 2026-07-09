//! `collect_needed_data_paths` must mirror last-match-wins unless semantics
//! (`compile_piecewise_rule` evaluates unless arms in reverse source order).

use lemma::DataOverlay;
use lemma::DataValueInput;
use lemma::DateGranularity;
use lemma::DateTimeValue;
use lemma::Engine;
use lemma::TimezoneValue;

const FILM_ACCESS: &str = r#"
spec premium_membership
uses lemma units
data start: date
data length: units.calendar
rule valid: now in start...start + length

spec film_access
uses premium_membership
data type: text
  -> option "rental"
  -> option "purchase"
data views_consumed: number
data premium_member: boolean
rule max_views: 3
  unless premium_membership.valid then 10
  unless premium_member then 5
rule can_view: no
  unless type is "rental" and views_consumed < max_views then yes
  unless type is "purchase" then yes
"#;

fn film_access_plan(engine: &Engine) -> lemma::ExecutionPlan {
    let now = DateTimeValue {
        year: 2027,
        month: 2,
        day: 14,
        hour: 12,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: Some(TimezoneValue {
            offset_hours: 0,
            offset_minutes: 0,
        }),
        granularity: DateGranularity::DateTime,
    };
    engine
        .get_plan(None, "film_access", Some(&now))
        .expect("film_access plan must build")
        .clone()
}

fn overlay_with(
    plan: &lemma::ExecutionPlan,
    engine: &Engine,
    values: impl IntoIterator<Item = (String, DataValueInput)>,
) -> DataOverlay {
    DataOverlay::resolve(plan, values.into_iter().collect(), engine.limits())
        .expect("overlay must resolve")
}

#[test]
fn schema_omits_membership_dates_when_premium_member_true() {
    let mut engine = Engine::new();
    engine
        .load(FILM_ACCESS, lemma::SourceType::Volatile)
        .expect("film_access spec must load");
    let plan = film_access_plan(&engine);
    let overlay = overlay_with(
        &plan,
        &engine,
        [
            (
                "type".to_string(),
                DataValueInput::convenience("rental".to_string()),
            ),
            (
                "views_consumed".to_string(),
                DataValueInput::convenience("6".to_string()),
            ),
            ("premium_member".to_string(), DataValueInput::Boolean(true)),
        ],
    );

    let schema = plan
        .schema_for_rules(&["can_view".to_string()], &overlay)
        .expect("schema must succeed");

    assert!(
        !schema.data.contains_key("premium_membership.start"),
        "start must not appear when last unless (premium_member) is true: {:?}",
        schema.data.keys().collect::<Vec<_>>()
    );
    assert!(
        !schema.data.contains_key("premium_membership.length"),
        "length must not appear when last unless (premium_member) is true: {:?}",
        schema.data.keys().collect::<Vec<_>>()
    );
}

#[test]
fn schema_includes_membership_dates_when_premium_member_false() {
    let mut engine = Engine::new();
    engine
        .load(FILM_ACCESS, lemma::SourceType::Volatile)
        .expect("film_access spec must load");
    let plan = film_access_plan(&engine);
    let overlay = overlay_with(
        &plan,
        &engine,
        [
            (
                "type".to_string(),
                DataValueInput::convenience("rental".to_string()),
            ),
            (
                "views_consumed".to_string(),
                DataValueInput::convenience("6".to_string()),
            ),
            ("premium_member".to_string(), DataValueInput::Boolean(false)),
        ],
    );

    let schema = plan
        .schema_for_rules(&["can_view".to_string()], &overlay)
        .expect("schema must succeed");

    assert!(
        schema.data.contains_key("premium_membership.start"),
        "start must appear when premium_member is false and valid unless may win"
    );
    assert!(
        schema.data.contains_key("premium_membership.length"),
        "length must appear when premium_member is false and valid unless may win"
    );
}

#[test]
fn schema_includes_membership_dates_when_premium_member_unknown() {
    let mut engine = Engine::new();
    engine
        .load(FILM_ACCESS, lemma::SourceType::Volatile)
        .expect("film_access spec must load");
    let plan = film_access_plan(&engine);
    let overlay = overlay_with(
        &plan,
        &engine,
        [
            (
                "type".to_string(),
                DataValueInput::convenience("rental".to_string()),
            ),
            (
                "views_consumed".to_string(),
                DataValueInput::convenience("6".to_string()),
            ),
        ],
    );

    let schema = plan
        .schema_for_rules(&["can_view".to_string()], &overlay)
        .expect("schema must succeed");

    assert!(
        schema.data.contains_key("premium_member"),
        "premium_member must appear when last unless outcome is unknown"
    );
    assert!(
        schema.data.contains_key("premium_membership.start"),
        "start must appear when premium_member is unknown"
    );
    assert!(
        schema.data.contains_key("premium_membership.length"),
        "length must appear when premium_member is unknown"
    );
}
