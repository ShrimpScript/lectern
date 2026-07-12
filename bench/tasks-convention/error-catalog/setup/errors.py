class PolicyError(Exception):
    """Raised to signal a rejected operation.

    `code` is a catalog string identifying why the operation was refused.
    """

    def __init__(self, code):
        self.code = code
        super().__init__(code)
