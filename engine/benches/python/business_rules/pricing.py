"""Port of engine/benches/specs/pricing.lemma."""

from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True, slots=True)
class Inputs:
    product_type: str
    quantity: Decimal
    unit_price: Decimal
    coupon_percent: Decimal
    loyalty_years: Decimal
    is_member: bool
    is_loyalty: bool
    is_tax_exempt: bool


@dataclass(frozen=True, slots=True)
class Outputs:
    is_standard: bool
    is_premium: bool
    is_luxury: bool
    volume_discount: Decimal
    tier_discount: Decimal
    member_discount: Decimal
    loyalty_discount: Decimal
    coupon_discount: Decimal
    combined_discount: Decimal
    subtotal: Decimal
    discount_amount: Decimal
    taxable: Decimal
    tax_rate: Decimal
    tax: Decimal
    total: Decimal


def build_inputs(raw: dict[str, str]) -> Inputs:
    return Inputs(
        product_type=raw["product_type"],
        quantity=Decimal(raw["quantity"]),
        unit_price=Decimal(raw["unit_price"]),
        coupon_percent=Decimal(raw["coupon_percent"]),
        loyalty_years=Decimal(raw["loyalty_years"]),
        is_member=raw["is_member"] == "true",
        is_loyalty=raw["is_loyalty"] == "true",
        is_tax_exempt=raw["is_tax_exempt"] == "true",
    )


def compute(inputs: Inputs) -> Outputs:
    is_standard = inputs.product_type == "standard"
    is_premium = inputs.product_type == "premium"
    is_luxury = inputs.product_type == "luxury"

    volume_discount = Decimal(0)
    if inputs.quantity >= 10:
        volume_discount = Decimal("0.05")
    if inputs.quantity >= 50:
        volume_discount = Decimal("0.10")
    if inputs.quantity >= 100:
        volume_discount = Decimal("0.15")

    tier_discount = Decimal(0)
    if is_premium:
        tier_discount = Decimal("0.05")
    if is_luxury:
        tier_discount = Decimal("0.15")

    member_discount = Decimal(0)
    if inputs.is_member:
        member_discount = Decimal("0.05")

    loyalty_discount = Decimal(0)
    if inputs.is_loyalty and inputs.loyalty_years >= 1:
        loyalty_discount = Decimal("0.03")
    if inputs.is_loyalty and inputs.loyalty_years >= 3:
        loyalty_discount = Decimal("0.06")
    if inputs.is_loyalty and inputs.loyalty_years >= 5:
        loyalty_discount = Decimal("0.10")

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

    tax_rate = Decimal("0.08")
    if inputs.is_tax_exempt:
        tax_rate = Decimal(0)

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
