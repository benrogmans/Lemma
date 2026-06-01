"""Port of engine/benches/specs/order_pipeline.lemma."""

from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True, slots=True)
class Inputs:
    customer_tier: str
    payment_method: str
    shipping_zone: str
    quantity: Decimal
    unit_price: Decimal
    package_weight: Decimal
    delivery_distance: Decimal
    loyalty_points: Decimal
    coupon_percent: Decimal
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
    subtotal: Decimal
    volume_discount_rate: Decimal
    tier_discount_rate: Decimal
    coupon_rate: Decimal
    first_time_rate: Decimal
    loyalty_rate: Decimal
    combined_discount_rate: Decimal
    discount_amount: Decimal
    net_price: Decimal
    base_shipping: Decimal
    distance_fee: Decimal
    zone_multiplier: Decimal
    fragile_fee: Decimal
    express_multiplier: Decimal
    hazardous_fee: Decimal
    gift_wrap_fee: Decimal
    total_shipping: Decimal
    pays_credit: bool
    pays_debit: bool
    pays_cash: bool
    pays_transfer: bool
    processing_fee: Decimal
    pre_tax_total: Decimal
    tax_rate: Decimal
    tax_amount: Decimal
    loyalty_credit_rate: Decimal
    loyalty_credit_earned: Decimal
    grand_total: Decimal
    meets_minimum: bool
    order_valid: bool
    order_valid_veto: str | None
    savings_total: Decimal
    savings_percent: Decimal
    is_high_value: bool
    is_express_eligible: bool
    order_summary: str


def build_inputs(raw: dict[str, str]) -> Inputs:
    return Inputs(
        customer_tier=raw["customer_tier"],
        payment_method=raw["payment_method"],
        shipping_zone=raw["shipping_zone"],
        quantity=Decimal(raw["quantity"]),
        unit_price=Decimal(raw["unit_price"]),
        package_weight=Decimal(raw["package_weight"]),
        delivery_distance=Decimal(raw["delivery_distance"]),
        loyalty_points=Decimal(raw["loyalty_points"]),
        coupon_percent=Decimal(raw["coupon_percent"]),
        is_fragile=raw["is_fragile"] == "true",
        is_express=raw["is_express"] == "true",
        is_hazardous=raw["is_hazardous"] == "true",
        is_gift=raw["is_gift"] == "true",
        is_first_time=raw["is_first_time"] == "true",
    )


def compute(inputs: Inputs) -> Outputs:
    is_standard = inputs.customer_tier == "standard"
    is_silver = inputs.customer_tier == "silver"
    is_gold = inputs.customer_tier == "gold"
    is_platinum = inputs.customer_tier == "platinum"

    subtotal = inputs.unit_price * inputs.quantity

    volume_discount_rate = Decimal(0)
    if inputs.quantity >= 5:
        volume_discount_rate = Decimal("0.03")
    if inputs.quantity >= 25:
        volume_discount_rate = Decimal("0.07")
    if inputs.quantity >= 100:
        volume_discount_rate = Decimal("0.12")
    if inputs.quantity >= 500:
        volume_discount_rate = Decimal("0.18")

    tier_discount_rate = Decimal(0)
    if is_silver:
        tier_discount_rate = Decimal("0.05")
    if is_gold:
        tier_discount_rate = Decimal("0.10")
    if is_platinum:
        tier_discount_rate = Decimal("0.15")

    coupon_rate = inputs.coupon_percent / 100

    first_time_rate = Decimal(0)
    if inputs.is_first_time:
        first_time_rate = Decimal("0.08")

    loyalty_rate = Decimal(0)
    if inputs.loyalty_points >= 1_000:
        loyalty_rate = Decimal("0.02")
    if inputs.loyalty_points >= 5_000:
        loyalty_rate = Decimal("0.05")
    if inputs.loyalty_points >= 25_000:
        loyalty_rate = Decimal("0.10")

    combined_discount_rate = (
        volume_discount_rate
        + tier_discount_rate
        + coupon_rate
        + first_time_rate
        + loyalty_rate
    )

    discount_amount = subtotal * combined_discount_rate
    net_price = subtotal - discount_amount

    base_shipping = Decimal(5)
    if inputs.package_weight > 1:
        base_shipping = Decimal(8)
    if inputs.package_weight > 5:
        base_shipping = Decimal(14)
    if inputs.package_weight > 20:
        base_shipping = Decimal(25)
    if inputs.package_weight > 50:
        base_shipping = Decimal(45)

    distance_fee = inputs.delivery_distance * Decimal("0.04")

    zone_multiplier = Decimal(1)
    if inputs.shipping_zone == "regional":
        zone_multiplier = Decimal("1.2")
    if inputs.shipping_zone == "national":
        zone_multiplier = Decimal("1.5")
    if inputs.shipping_zone == "intl":
        zone_multiplier = Decimal("2.5")

    fragile_fee = Decimal(0)
    if inputs.is_fragile:
        fragile_fee = Decimal(8)

    express_multiplier = Decimal(1)
    if inputs.is_express:
        express_multiplier = Decimal(2)

    hazardous_fee = Decimal(0)
    if inputs.is_hazardous:
        hazardous_fee = Decimal(20)

    gift_wrap_fee = Decimal(0)
    if inputs.is_gift:
        gift_wrap_fee = Decimal(5)

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

    processing_fee = Decimal(0)
    if pays_credit:
        processing_fee = net_price * Decimal("0.025")
    if pays_cash:
        processing_fee = Decimal(2)
    if pays_transfer:
        processing_fee = Decimal(1)

    pre_tax_total = net_price + total_shipping + processing_fee

    tax_rate = Decimal("0.08")
    if inputs.shipping_zone == "intl":
        tax_rate = Decimal(0)

    tax_amount = pre_tax_total * tax_rate

    loyalty_credit_rate = Decimal("0.01")
    if is_silver:
        loyalty_credit_rate = Decimal("0.02")
    if is_gold:
        loyalty_credit_rate = Decimal("0.03")
    if is_platinum:
        loyalty_credit_rate = Decimal("0.05")

    loyalty_credit_earned = pre_tax_total * loyalty_credit_rate
    grand_total = pre_tax_total + tax_amount

    meets_minimum = subtotal >= 25

    order_valid = meets_minimum
    order_valid_veto: str | None = None
    if not meets_minimum:
        order_valid = False
        order_valid_veto = "Order subtotal below the minimum of 25"

    savings_total = discount_amount
    savings_percent = savings_total / subtotal

    is_high_value = grand_total > 500

    is_express_eligible = False
    if total_shipping >= 25:
        is_express_eligible = True
    if inputs.is_express:
        is_express_eligible = True

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
