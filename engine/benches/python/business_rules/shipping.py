"""Port of engine/benches/specs/shipping.lemma."""

from dataclasses import dataclass

from business_rules.rational import Rational, parse_rational

TERMINAL_RULE = "total"

ZERO = Rational(0)
TWO = Rational(2)
FIVE = Rational(5)
TWELVE = Rational(12)
FIFTEEN = Rational(15)
THIRTY = Rational(30)
PCT_10 = Rational("0.10")


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


def _eval_closure(
    inputs: Inputs,
) -> tuple[Rational, Rational, Rational, Rational, Rational, Rational]:
    base_rate = FIVE
    if inputs.destination == "express":
        base_rate = FIFTEEN
    if inputs.destination == "international":
        base_rate = THIRTY

    weight_fee = ZERO
    if inputs.weight > 1:
        weight_fee = TWO
    if inputs.weight > 5:
        weight_fee = FIVE
    if inputs.weight > 20:
        weight_fee = TWELVE

    member_discount = ZERO
    if inputs.is_member:
        member_discount = PCT_10

    subtotal = base_rate + weight_fee
    discount_amount = subtotal * member_discount
    total = subtotal - discount_amount

    return (
        base_rate,
        weight_fee,
        member_discount,
        subtotal,
        discount_amount,
        total,
    )


def compute(inputs: Inputs) -> Outputs:
    (
        base_rate,
        weight_fee,
        member_discount,
        subtotal,
        discount_amount,
        total,
    ) = _eval_closure(inputs)
    return Outputs(
        base_rate=base_rate,
        weight_fee=weight_fee,
        member_discount=member_discount,
        subtotal=subtotal,
        discount_amount=discount_amount,
        total=total,
    )


def compute_terminal(inputs: Inputs) -> Rational:
    return _eval_closure(inputs)[5]
