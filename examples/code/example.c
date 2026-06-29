/* A small C example: a singly-linked list with push and print. */

#include <stdio.h>
#include <stdlib.h>

struct Node {
    int   value;
    struct Node *next;
};

struct Node *push(struct Node *head, int value) {
    struct Node *n = malloc(sizeof(*n));
    if (!n) { perror("malloc"); exit(1); }
    n->value = value;
    n->next  = head;
    return n;
}

void print_list(const struct Node *head) {
    for (const struct Node *n = head; n != NULL; n = n->next) {
        printf("%d%s", n->value, n->next ? " -> " : "\n");
    }
}

int main(void) {
    struct Node *list = NULL;
    int values[] = {1, 2, 3, 4, 5};
    for (int i = 0; i < 5; i++) {
        list = push(list, values[i]);
    }
    print_list(list);
    return 0;
}
