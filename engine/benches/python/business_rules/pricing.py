"""Port of engine/benches/specs/pricing.lemma."""

from dataclasses import dataclass

from business_rules.rational import Rational, parse_rational

TERMINAL_RULE = "total"

ZERO = Rational(0)
PCT_3 = Rational("0.03")
PCT_5 = Rational("0.05")
PCT_6 = Rational("0.06")
PCT_8 = Rational("0.08")
PCT_10 = Rational("0.10")
PCT_15 = Rational("0.15")


@dataclass(frozen=True, slots=True)
class Inputs:
    product_type: str
    quantity: Rational
    unit_price: Rational
    coupon_percent: Rational
    loyalty_years: Rational
    is_member: bool
    is_loyalty: bool
    is_tax_exempt: bool


@dataclass(frozen=True, slots=True)
class Outputs:
    is_standard: bool
    is_premium: bool
    is_luxury: bool
    volume_discount: Rational
    tier_discount: Rational
    member_discount: Rational
    loyalty_discount: Rational
    coupon_discount: Rational
    combined_discount: Rational
    subtotal: Rational
    discount_amount: Rational
    taxable: Rational
    tax_rate: Rational
    tax: Rational
    total: Rational


def build_inputs(raw: dict[str, str]) -> Inputs:
    return Inputs(
        product_type=raw["product_type"],
        quantity=parse_rational(raw["quantity"]),
        unit_price=parse_rational(raw["unit_price"]),
        coupon_percent=parse_rational(raw["coupon_percent"]),
        loyalty_years=parse_rational(raw["loyalty_years"]),
        is_member=raw["is_member"] == "true",
        is_loyalty=raw["is_loyalty"] == "true",
        is_tax_exempt=raw["is_tax_exempt"] == "true",
    )


def _eval_closure(
    inputs: Inputs,
) -> tuple[
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
]:
    is_premium = inputs.product_type == "premium"
    is_luxury = inputs.product_type == "luxury"

    volume_discount = ZERO
    if inputs.quantity >= 10:
        volume_discount = PCT_5
    if inputs.quantity >= 50:
        volume_discount = PCT_10
    if inputs.quantity >= 100:
        volume_discount = PCT_15

    tier_discount = ZERO
    if is_premium:
        tier_discount = PCT_5
    if is_luxury:
        tier_discount = PCT_15

    member_discount = ZERO
    if inputs.is_member:
        member_discount = PCT_5

    loyalty_discount = ZERO
    if inputs.is_loyalty and inputs.loyalty_years >= 1:
        loyalty_discount = PCT_3
    if inputs.is_loyalty and inputs.loyalty_years >= 3:
        loyalty_discount = PCT_6
    if inputs.is_loyalty and inputs.loyalty_years >= 5:
        loyalty_discount = PCT_10

    coupon_discount = inputs.coupon_percent / 100

    combined_discount = (
        volume_discount
        + tier_discount
        + member_discount
        + loyalty_discount
        + coupon_discount
    )

    subtotal = inputs.unit_price * inputs.quantity
    discount_amount = subtotal * combined_discount
    taxable = subtotal - discount_amount

    tax_rate = PCT_8
    if inputs.is_tax_exempt:
        tax_rate = ZERO

    tax = taxable * tax_rate
    total = taxable + tax

    return (
        is_premium,
        is_luxury,
        volume_discount,
        tier_discount,
        member_discount,
        loyalty_discount,
        coupon_discount,
        combined_discount,
        subtotal,
        discount_amount,
        taxable,
        tax_rate,
        tax,
        total,
    )


def compute(inputs: Inputs) -> Outputs:
    (
        is_premium,
        is_luxury,
        volume_discount,
        tier_discount,
        member_discount,
        loyalty_discount,
        coupon_discount,
        combined_discount,
        subtotal,
        discount_amount,
        taxable,
        tax_rate,
        tax,
        total,
    ) = _eval_closure(inputs)
    return Outputs(
        is_standard=inputs.product_type == "standard",
        is_premium=is_premium,
        is_luxury=is_luxury,
        volume_discount=volume_discount,
        tier_discount=tier_discount,
        member_discount=member_discount,
        loyalty_discount=loyalty_discount,
        coupon_discount=coupon_discount,
        combined_discount=combined_discount,
        subtotal=subtotal,
        discount_amount=discount_amount,
        taxable=taxable,
        tax_rate=tax_rate,
        tax=tax,
        total=total,
    )


def compute_terminal(inputs: Inputs) -> Rational:
    return _eval_closure(inputs)[13]
