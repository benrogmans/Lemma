"""Port of engine/benches/specs/pricing.lemma."""

from dataclasses import dataclass

from business_rules.rational import Rational, parse_rational

TERMINAL_RULE = "total"


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


def compute(inputs: Inputs) -> Outputs:
    is_standard = inputs.product_type == "standard"
    is_premium = inputs.product_type == "premium"
    is_luxury = inputs.product_type == "luxury"

    volume_discount = Rational(0)
    if inputs.quantity >= 10:
        volume_discount = Rational("0.05")
    if inputs.quantity >= 50:
        volume_discount = Rational("0.10")
    if inputs.quantity >= 100:
        volume_discount = Rational("0.15")

    tier_discount = Rational(0)
    if is_premium:
        tier_discount = Rational("0.05")
    if is_luxury:
        tier_discount = Rational("0.15")

    member_discount = Rational(0)
    if inputs.is_member:
        member_discount = Rational("0.05")

    loyalty_discount = Rational(0)
    if inputs.is_loyalty and inputs.loyalty_years >= 1:
        loyalty_discount = Rational("0.03")
    if inputs.is_loyalty and inputs.loyalty_years >= 3:
        loyalty_discount = Rational("0.06")
    if inputs.is_loyalty and inputs.loyalty_years >= 5:
        loyalty_discount = Rational("0.10")

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

    tax_rate = Rational("0.08")
    if inputs.is_tax_exempt:
        tax_rate = Rational(0)

    tax = taxable * tax_rate
    total = taxable + tax

    return Outputs(
        is_standard=is_standard,
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
    return compute(inputs).total
