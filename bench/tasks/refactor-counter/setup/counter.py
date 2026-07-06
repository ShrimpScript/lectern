"""A tiny counter with module-level state (to be refactored into a class)."""

_count = 0


def increment():
    global _count
    _count += 1
    return _count


def decrement():
    global _count
    _count -= 1
    return _count


def value():
    return _count
