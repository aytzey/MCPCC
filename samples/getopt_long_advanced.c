/**
 * getopt_long_advanced.c - Advanced getopt_long sample for mcpcc testing
 *
 * Features tested:
 * - Repeatable option (-v -v -v for verbose level)
 * - optional_argument (--level[=N], --format[=FMT])
 * - Standard required_argument and no_argument options
 */
#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
  // Print all received arguments for test verification
  for (int i = 1; i < argc; i++) {
    printf("ARG:%s\n", argv[i]);
  }

  int opt = 0;
  int verbose_level = 0;
  const char *output = NULL;
  const char *input = NULL;
  const char *level = NULL;
  const char *format = NULL;
  int dry_run = 0;

  static struct option long_options[] = {
      {"verbose", no_argument, 0, 'v'},
      {"output", required_argument, 0, 'o'},
      {"input", required_argument, 0, 'i'},
      {"level", optional_argument, 0, 'l'},
      {"format", optional_argument, 0, 'f'},
      {"dry-run", no_argument, 0, 'n'},
      {0, 0, 0, 0},
  };

  // optstring: v (no arg, repeatable), o: (required), i: (required),
  // l:: (optional), f:: (optional), n (no arg)
  while ((opt = getopt_long(argc, argv, "vo:i:l::f::n", long_options, 0)) !=
         -1) {
    switch (opt) {
    case 'v':
      verbose_level++;
      break;
    case 'o':
      output = optarg;
      break;
    case 'i':
      input = optarg;
      break;
    case 'l':
      level = optarg ? optarg : "default";
      break;
    case 'f':
      format = optarg ? optarg : "auto";
      break;
    case 'n':
      dry_run = 1;
      break;
    default:
      fprintf(stderr, "Usage: %s [-v...] -o FILE [-i FILE] [--level[=N]] "
                      "[--format[=FMT]] [-n] [args...]\n",
              argv[0]);
      return 2;
    }
  }

  if (!output) {
    fprintf(stderr, "missing --output\n");
    return 2;
  }

  // Print parsed values for verification
  printf("VERBOSE:%d\n", verbose_level);
  printf("OUTPUT:%s\n", output);
  if (input)
    printf("INPUT:%s\n", input);
  if (level)
    printf("LEVEL:%s\n", level);
  if (format)
    printf("FORMAT:%s\n", format);
  if (dry_run)
    printf("DRY_RUN:1\n");

  // Print remaining positional arguments
  for (int i = optind; i < argc; i++) {
    printf("POSITIONAL:%s\n", argv[i]);
  }

  return 0;
}
