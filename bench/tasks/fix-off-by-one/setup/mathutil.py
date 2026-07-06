def sum_to(n):
    # BUG: excludes n; should sum 1..n inclusive.
    return sum(range(1, n))
