"""A small inventory ledger. Quantities are integers; money is integer cents."""


class Inventory:
    def __init__(self):
        self._items = {}  # name -> {"qty": int, "price_cents": int}

    def add(self, name, qty, price_cents):
        if qty <= 0:
            raise ValueError("qty must be positive")
        cur = self._items.get(name)
        if cur is None:
            self._items[name] = {"qty": qty, "price_cents": price_cents}
        else:
            # BUG: overwrites instead of accumulating
            cur["qty"] = qty
            cur["price_cents"] = price_cents

    def remove(self, name, qty):
        cur = self._items.get(name)
        if cur is None:
            raise KeyError(name)
        # BUG: allows going negative (should raise ValueError, leave state unchanged)
        cur["qty"] -= qty
        if cur["qty"] == 0:
            del self._items[name]

    def total_value_cents(self):
        # BUG: off-by-one — skips the first item due to slicing
        vals = [v["qty"] * v["price_cents"] for v in list(self._items.values())[1:]]
        return sum(vals)

    def names(self):
        return sorted(self._items)
