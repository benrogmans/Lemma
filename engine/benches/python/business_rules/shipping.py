"""Port of engine/benches/specs/shipping.lemma."""

from dataclasses import dataclass

from business_rules.rational import Rational, parse_rational


@dataclass(frozen=True, slots=True)
class Inputs:
    weight: Rational
    destination: str
    is_member: bool


@dataclass(frozen=True, slots=True)
class Outputs:
    base_rate: Rational
    weight_fee: Rational
    member_discount: Rational
    subtotal: Rational
    discount_amount: Rational
    total: Rational


def build_inputs(raw: dict[str, str]) -> Inputs:
    return Inputs(
        weight=parse_rational(raw["weight"]),
        destination=raw["destination"],
        is_member=raw["is_member"] == "true",
    )


def compute(inputs: Inputs) -> Outputs:
    base_rate = Rational(5)
    if inputs.destination == "express":
        base_rate = Rational(15)
    if inputs.destination == "international":
        base_rate = Rational(30)

    weight_fee = Rational(0)
    if inputs.weight > 1:
        weight_fee = Rational(2)
    if inputs.weight > 5:
        weight_fee = Rational(5)
    if inputs.weight > 20:
        weight_fee = Rational(12)

    member_discount = Rational(0)
    if inputs.is_member:
        member_discount = Rational("0.10")

    subtotal = base_rate + weight_fee
    discount_amount = subtotal * member_discount
    total = subtotal - discount_amount

    return Outputs(
        base_rate=base_rate,
        weight_fee=weight_fee,
        member_discount=member_discount,
        subtotal=subtotal,
        discount_amount=discount_amount,
        total=total,
    )
