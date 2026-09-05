"""Port of engine/benches/specs/order_pipeline.lemma."""

from dataclasses import dataclass

from business_rules.rational import Rational, parse_rational

TERMINAL_RULE = "grand_total"

ZERO = Rational(0)
ONE = Rational(1)
TWO = Rational(2)
FIVE = Rational(5)
EIGHT = Rational(8)
FOURTEEN = Rational(14)
TWENTY = Rational(20)
TWENTY_FIVE = Rational(25)
FORTY_FIVE = Rational(45)
PCT_1 = Rational("0.01")
PCT_2 = Rational("0.02")
PCT_2_5 = Rational("0.025")
PCT_3 = Rational("0.03")
PCT_4 = Rational("0.04")
PCT_5 = Rational("0.05")
PCT_7 = Rational("0.07")
PCT_8 = Rational("0.08")
PCT_10 = Rational("0.10")
PCT_12 = Rational("0.12")
PCT_15 = Rational("0.15")
PCT_18 = Rational("0.18")
MULT_1_2 = Rational("1.2")
MULT_1_5 = Rational("1.5")
MULT_2_5 = Rational("2.5")


@dataclass(frozen=True, slots=True)
class Inputs:
    customer_tier: str
    payment_method: str
    shipping_zone: str
    quantity: Rational
    unit_price: Rational
    package_weight: Rational
    delivery_distance: Rational
    loyalty_points: Rational
    coupon_percent: Rational
    is_fragile: bool
    is_express: bool
    is_hazardous: bool
    is_gift: bool
    is_first_time: bool


@dataclass(frozen=True, slots=True)
class Outputs:
    is_standard: bool
    is_silver: bool
    is_gold: bool
    is_platinum: bool
    subtotal: Rational
    volume_discount_rate: Rational
    tier_discount_rate: Rational
    coupon_rate: Rational
    first_time_rate: Rational
    loyalty_rate: Rational
    combined_discount_rate: Rational
    discount_amount: Rational
    net_price: Rational
    base_shipping: Rational
    distance_fee: Rational
    zone_multiplier: Rational
    fragile_fee: Rational
    express_multiplier: Rational
    hazardous_fee: Rational
    gift_wrap_fee: Rational
    total_shipping: Rational
    pays_credit: bool
    pays_debit: bool
    pays_cash: bool
    pays_transfer: bool
    processing_fee: Rational
    pre_tax_total: Rational
    tax_rate: Rational
    tax_amount: Rational
    loyalty_credit_rate: Rational
    loyalty_credit_earned: Rational
    grand_total: Rational
    meets_minimum: bool
    order_valid: bool
    order_valid_veto: str | None
    savings_total: Rational
    savings_percent: Rational
    is_high_value: bool
    is_express_eligible: bool
    order_summary: str


def build_inputs(raw: dict[str, str]) -> Inputs:
    return Inputs(
        customer_tier=raw["customer_tier"],
        payment_method=raw["payment_method"],
        shipping_zone=raw["shipping_zone"],
        quantity=parse_rational(raw["quantity"]),
        unit_price=parse_rational(raw["unit_price"]),
        package_weight=parse_rational(raw["package_weight"]),
        delivery_distance=parse_rational(raw["delivery_distance"]),
        loyalty_points=parse_rational(raw["loyalty_points"]),
        coupon_percent=parse_rational(raw["coupon_percent"]),
        is_fragile=raw["is_fragile"] == "true",
        is_express=raw["is_express"] == "true",
        is_hazardous=raw["is_hazardous"] == "true",
        is_gift=raw["is_gift"] == "true",
        is_first_time=raw["is_first_time"] == "true",
    )


def _eval_closure(
    inputs: Inputs,
) -> tuple[
    bool,
    bool,
    bool,
    bool,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
    bool,
    bool,
    bool,
    bool,
    Rational,
    Rational,
    Rational,
    Rational,
    Rational,
]:
    is_standard = inputs.customer_tier == "standard"
    is_silver = inputs.customer_tier == "silver"
    is_gold = inputs.customer_tier == "gold"
    is_platinum = inputs.customer_tier == "platinum"

    subtotal = inputs.unit_price * inputs.quantity

    volume_discount_rate = ZERO
    if inputs.quantity >= 5:
        volume_discount_rate = PCT_3
    if inputs.quantity >= 25:
        volume_discount_rate = PCT_7
    if inputs.quantity >= 100:
        volume_discount_rate = PCT_12
    if inputs.quantity >= 500:
        volume_discount_rate = PCT_18

    tier_discount_rate = ZERO
    if is_silver:
        tier_discount_rate = PCT_5
    if is_gold:
        tier_discount_rate = PCT_10
    if is_platinum:
        tier_discount_rate = PCT_15

    coupon_rate = inputs.coupon_percent / 100

    first_time_rate = ZERO
    if inputs.is_first_time:
        first_time_rate = PCT_8

    loyalty_rate = ZERO
    if inputs.loyalty_points >= 1_000:
        loyalty_rate = PCT_2
    if inputs.loyalty_points >= 5_000:
        loyalty_rate = PCT_5
    if inputs.loyalty_points >= 25_000:
        loyalty_rate = PCT_10

    combined_discount_rate = (
        volume_discount_rate
        + tier_discount_rate
        + coupon_rate
        + first_time_rate
        + loyalty_rate
    )

    discount_amount = subtotal * combined_discount_rate
    net_price = subtotal - discount_amount

    base_shipping = FIVE
    if inputs.package_weight > 1:
        base_shipping = EIGHT
    if inputs.package_weight > 5:
        base_shipping = FOURTEEN
    if inputs.package_weight > 20:
        base_shipping = TWENTY_FIVE
    if inputs.package_weight > 50:
        base_shipping = FORTY_FIVE

    distance_fee = inputs.delivery_distance * PCT_4

    zone_multiplier = ONE
    if inputs.shipping_zone == "regional":
        zone_multiplier = MULT_1_2
    if inputs.shipping_zone == "national":
        zone_multiplier = MULT_1_5
    if inputs.shipping_zone == "intl":
        zone_multiplier = MULT_2_5

    fragile_fee = ZERO
    if inputs.is_fragile:
        fragile_fee = EIGHT

    express_multiplier = ONE
    if inputs.is_express:
        express_multiplier = TWO

    hazardous_fee = ZERO
    if inputs.is_hazardous:
        hazardous_fee = TWENTY

    gift_wrap_fee = ZERO
    if inputs.is_gift:
        gift_wrap_fee = FIVE

    total_shipping = (
        (
            base_shipping
            + distance_fee
            + fragile_fee
            + hazardous_fee
            + gift_wrap_fee
        )
        * zone_multiplier
        * express_multiplier
    )

    pays_credit = inputs.payment_method == "credit"
    pays_debit = inputs.payment_method == "debit"
    pays_cash = inputs.payment_method == "cash"
    pays_transfer = inputs.payment_method == "transfer"

    processing_fee = ZERO
    if pays_credit:
        processing_fee = net_price * PCT_2_5
    if pays_cash:
        processing_fee = TWO
    if pays_transfer:
        processing_fee = ONE

    pre_tax_total = net_price + total_shipping + processing_fee

    tax_rate = PCT_8
    if inputs.shipping_zone == "intl":
        tax_rate = ZERO

    tax_amount = pre_tax_total * tax_rate
    grand_total = pre_tax_total + tax_amount

    return (
        is_standard,
        is_silver,
        is_gold,
        is_platinum,
        subtotal,
        volume_discount_rate,
        tier_discount_rate,
        coupon_rate,
        first_time_rate,
        loyalty_rate,
        combined_discount_rate,
        discount_amount,
        net_price,
        base_shipping,
        distance_fee,
        zone_multiplier,
        fragile_fee,
        express_multiplier,
        hazardous_fee,
        gift_wrap_fee,
        total_shipping,
        pays_credit,
        pays_debit,
        pays_cash,
        pays_transfer,
        processing_fee,
        pre_tax_total,
        tax_rate,
        tax_amount,
        grand_total,
    )


def compute_terminal(inputs: Inputs) -> Rational:
    return _eval_closure(inputs)[29]


def compute(inputs: Inputs) -> Outputs:
    (
        is_standard,
        is_silver,
        is_gold,
        is_platinum,
        subtotal,
        volume_discount_rate,
        tier_discount_rate,
        coupon_rate,
        first_time_rate,
        loyalty_rate,
        combined_discount_rate,
        discount_amount,
        net_price,
        base_shipping,
        distance_fee,
        zone_multiplier,
        fragile_fee,
        express_multiplier,
        hazardous_fee,
        gift_wrap_fee,
        total_shipping,
        pays_credit,
        pays_debit,
        pays_cash,
        pays_transfer,
        processing_fee,
        pre_tax_total,
        tax_rate,
        tax_amount,
        grand_total,
    ) = _eval_closure(inputs)

    loyalty_credit_rate = PCT_1
    if is_silver:
        loyalty_credit_rate = PCT_2
    if is_gold:
        loyalty_credit_rate = PCT_3
    if is_platinum:
        loyalty_credit_rate = PCT_5

    loyalty_credit_earned = pre_tax_total * loyalty_credit_rate

    meets_minimum = subtotal >= 25

    order_valid = meets_minimum
    order_valid_veto: str | None = None
    if not meets_minimum:
        order_valid = False
        order_valid_veto = "Order subtotal below the minimum of 25"

    savings_total = discount_amount
    savings_percent = discount_amount / subtotal

    is_high_value = grand_total > 500

    is_express_eligible = total_shipping >= 25 or inputs.is_express

    order_summary = "standard order"
    if is_high_value:
        order_summary = "high value order"
    if inputs.is_first_time:
        order_summary = "first-time customer"
    if is_platinum:
        order_summary = "platinum customer order"
    if inputs.is_express:
        order_summary = "express order"

    return Outputs(
        is_standard=is_standard,
        is_silver=is_silver,
        is_gold=is_gold,
        is_platinum=is_platinum,
        subtotal=subtotal,
        volume_discount_rate=volume_discount_rate,
        tier_discount_rate=tier_discount_rate,
        coupon_rate=coupon_rate,
        first_time_rate=first_time_rate,
        loyalty_rate=loyalty_rate,
        combined_discount_rate=combined_discount_rate,
        discount_amount=discount_amount,
        net_price=net_price,
        base_shipping=base_shipping,
        distance_fee=distance_fee,
        zone_multiplier=zone_multiplier,
        fragile_fee=fragile_fee,
        express_multiplier=express_multiplier,
        hazardous_fee=hazardous_fee,
        gift_wrap_fee=gift_wrap_fee,
        total_shipping=total_shipping,
        pays_credit=pays_credit,
        pays_debit=pays_debit,
        pays_cash=pays_cash,
        pays_transfer=pays_transfer,
        processing_fee=processing_fee,
        pre_tax_total=pre_tax_total,
        tax_rate=tax_rate,
        tax_amount=tax_amount,
        loyalty_credit_rate=loyalty_credit_rate,
        loyalty_credit_earned=loyalty_credit_earned,
        grand_total=grand_total,
        meets_minimum=meets_minimum,
        order_valid=order_valid,
        order_valid_veto=order_valid_veto,
        savings_total=savings_total,
        savings_percent=savings_percent,
        is_high_value=is_high_value,
        is_express_eligible=is_express_eligible,
        order_summary=order_summary,
    )
