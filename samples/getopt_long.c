#include <getopt.h>
#include <stdio.h>

int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    printf("ARG:%s\n", argv[i]);
  }

  int opt = 0;
  int verbose = 0;
  const char *output = 0;
  const char *color = 0;

  static struct option long_options[] = {
      {"verbose", no_argument, 0, 'v'},
      {"output", required_argument, 0, 'o'},
      {"color", optional_argument, 0, 'c'},
      {0, 0, 0, 0},
  };

  while ((opt = getopt_long(argc, argv, "vo:c::", long_options, 0)) != -1) {
    switch (opt) {
    case 'v':
      verbose = 1;
      break;
    case 'o':
      output = optarg;
      break;
    case 'c':
      color = optarg ? optarg : "";
      break;
    default:
      break;
    }
  }

  if (!output) {
    fprintf(stderr, "missing --output\n");
    return 2;
  }

  (void)verbose;
  (void)color;
  return 0;
}

