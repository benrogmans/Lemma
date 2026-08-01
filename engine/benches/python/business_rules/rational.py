"""Exact rational arithmetic for Lemma benchmark ports.

Mirrors Lemma's internal arbitrary-precision rational model: compute stays in ℚ;
decimal strings are produced only at the output boundary.
"""

from decimal import Decimal
from fractions import Fraction

Rational = Fraction


def parse_rational(text: str) -> Rational:
    """Lift a fixture decimal string into an exact rational."""
    return Rational(text)


def rational_to_decimal_string(value: Rational) -> str:
    """Convert a rational to a decimal display string (Lemma API decimal string format)."""
    if value.denominator == 0:
        raise ZeroDivisionError("BUG: rational with zero denominator")
    if value.numerator == 0:
        return "0"
    decimal = (Decimal(value.numerator) / Decimal(value.denominator)).normalize()
    if decimal == decimal.to_integral_value():
        return format(decimal.quantize(Decimal(1)), "f")
    return format(decimal, "f")
