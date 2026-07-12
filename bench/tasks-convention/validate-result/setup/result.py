from collections import namedtuple

# A validation outcome used across this project. `code` is a catalog string.
Result = namedtuple("Result", ["ok", "code"])
