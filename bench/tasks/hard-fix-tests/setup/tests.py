"""Do not modify this file. Fix inventory.py until this passes."""
from inventory import Inventory


def check(cond, msg):
    if not cond:
        raise AssertionError(msg)


inv = Inventory()
inv.add("bolt", 10, 5)
inv.add("bolt", 5, 5)  # accumulate, same price
check(inv.total_value_cents() == 75, "add should accumulate qty (15 * 5 = 75)")

inv.add("nut", 4, 25)
check(inv.total_value_cents() == 75 + 100, "total must include every item")

try:
    inv.remove("bolt", 99)
    check(False, "removing more than held must raise ValueError")
except ValueError:
    pass
check(inv.total_value_cents() == 175, "failed remove must not change state")

inv.remove("nut", 4)
check(inv.names() == ["bolt"], "empty items are dropped")

try:
    inv.remove("ghost", 1)
    check(False, "removing unknown item must raise KeyError")
except KeyError:
    pass

print("all tests passed")
