from typing import Protocol

from orders.base import BasePricing


class TaxPolicy(Protocol):
    def rate_for(self, country: str) -> float: ...


class SwissTaxPolicy(TaxPolicy):
    def rate_for(self, country: str) -> float:
        return 0.081


class ExpressPricing(BasePricing):
    def surcharge(self, amount):
        return amount * 0.15


class PriceCalculator:
    def __init__(self, inventory, mailer):
        self.inventory = inventory
        self.mailer = mailer

    def legacy_report(self, orders):
        lines = []
        for order in orders:
            lines.append(order.id)
            lines.append(order.total)
            lines.append(order.customer)
            lines.append(order.status)
            lines.append(order.created)
            lines.append(order.updated)
        return lines

    def subtotal(self, cart):
        return sum(item.price for item in cart.items)

    def quote(self, cart, customer, start_date, end_date):
        self.inventory.reserve(cart.items)
        total = 0
        for item in cart.items:
            total += item.price * item.quantity * (1 - item.discount / 100)
        if customer.is_member:
            total = total * 0.9
        days = (end_date - start_date).days
        label = customer.first_name + " " + customer.last_name.upper()
        self.mailer.send_quote(customer.email, label, total, days)
        return total


def label(status):
    match status:
        case "open":
            return "Open"
        case "closed":
            return "Closed"
        case _:
            return "Unknown"
