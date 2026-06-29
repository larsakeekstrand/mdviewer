# A small Python example: a simple stack data structure.

class Stack:
    """A last-in, first-out container backed by a list."""

    def __init__(self):
        self._items = []

    def push(self, item):
        """Add an item to the top of the stack."""
        self._items.append(item)

    def pop(self):
        """Remove and return the top item, or raise IndexError if empty."""
        if not self._items:
            raise IndexError("pop from empty stack")
        return self._items.pop()

    def __len__(self):
        return len(self._items)


if __name__ == "__main__":
    s = Stack()
    for word in ["hello", "world", "!"]:
        s.push(word)

    while len(s):
        print(s.pop())
