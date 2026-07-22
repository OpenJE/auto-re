/* hello.c - minimal fixture binary for IDA ingest end-to-end testing.
 *
 * Expected function count: 4 (add, multiply, greet, main).
 * This constant is asserted in the integration test.
 */
#include <stdio.h>

int add(int a, int b) {
    return a + b;
}

int multiply(int a, int b) {
    return a * b;
}

void greet(const char *name) {
    printf("Hello, %s!\n", name);
}

int main(int argc, char **argv) {
    int sum = add(1, 2);
    int product = multiply(3, 4);
    greet("world");
    (void)argc;
    (void)argv;
    return sum + product;
}
