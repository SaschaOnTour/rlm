"""A sample Python module for testing the parser."""


class Config:
    """Configuration class."""

    def __init__(self, name: str, value: int):
        self.name = name
        self.value = value

    def display(self) -> str:
        return f"{self.name}: {self.value}"

    def _internal(self):
        pass


def helper(x: int) -> int:
    return x * 2


def _private_fn():
    cfg = Config("test", 42)
    result = helper(10)
    print(cfg.display())
    return result
