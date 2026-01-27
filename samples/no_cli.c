/**
 * no_cli.c - Sample with no CLI parsing for mcpcc testing
 *
 * This program has no argp or getopt usage. mcpcc should only generate
 * a run_raw tool for it (no structured tool).
 */
#include <stdio.h>

int main(int argc, char **argv) {
  printf("no_cli sample program\n");

  // Print all received arguments for test verification
  for (int i = 1; i < argc; i++) {
    printf("ARG:%s\n", argv[i]);
  }

  // Simple echo behavior
  if (argc > 1) {
    printf("First argument: %s\n", argv[1]);
    printf("Total arguments: %d\n", argc - 1);
  } else {
    printf("No arguments provided\n");
  }

  return 0;
}
