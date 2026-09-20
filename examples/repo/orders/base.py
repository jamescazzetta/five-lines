class BasePricing:
    def __init__(self, tax_rate):
        self.tax_rate = tax_rate

    def with_tax(self, amount):
        return amount * (1 + self.tax_rate)
