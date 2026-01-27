#include "calc.h"

#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>

static void usage(const char *prog) {
  fprintf(stderr,
          "Usage: %s [-e EXPR]\n\n"
          "Options:\n"
          "  -e, --expr EXPR   Evaluate EXPR (otherwise read from stdin)\n",
          prog);
}

static char *read_all_stdin(void) {
  size_t cap = 1024;
  size_t len = 0;
  char *buf = (char *)malloc(cap);
  if (!buf) return NULL;

  int ch;
  while ((ch = fgetc(stdin)) != EOF) {
    if (len + 1 >= cap) {
      cap *= 2;
      char *nb = (char *)realloc(buf, cap);
      if (!nb) {
        free(buf);
        return NULL;
      }
      buf = nb;
    }
    buf[len++] = (char)ch;
  }

  buf[len] = '\0';
  return buf;
}

int main(int argc, char **argv) {
  const char *expr = NULL;

  static struct option long_opts[] = {
      {"expr", required_argument, 0, 'e'},
      {0, 0, 0, 0},
  };

  for (;;) {
    int c = getopt_long(argc, argv, "e:", long_opts, NULL);
    if (c == -1) break;

    switch (c) {
      case 'e':
        expr = optarg;
        break;
      default:
        usage(argv[0]);
        return 2;
    }
  }

  char *owned = NULL;
  if (!expr) {
    owned = read_all_stdin();
    if (!owned) {
      fprintf(stderr, "failed to read stdin\n");
      return 1;
    }
    expr = owned;
  }

  double out = 0.0;
  int rc = calc_eval_double(expr, &out);
  if (rc != 0) {
    fprintf(stderr, "parse error (rc=%d)\n", rc);
    free(owned);
    return 1;
  }

  // Use %g: prints integers without trailing .0, but supports floats.
  printf("%g\n", out);
  free(owned);
  return 0;
}
