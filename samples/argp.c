#include <argp.h>
#include <stdio.h>

struct arguments {
  const char *output;
  const char *color;
  int verbose;
};

static struct argp_option options[] = {
    {"output", 'o', "FILE", 0, "Write output to FILE", 0},
    {"color", 'c', "WHEN", OPTION_ARG_OPTIONAL, "Colorize output (optional arg)", 0},
    {"verbose", 'v', 0, 0, "Verbose logging", 0},
    {0},
};

static error_t parse_opt(int key, char *arg, struct argp_state *state) {
  struct arguments *args = state->input;
  switch (key) {
  case 'o':
    args->output = arg;
    return 0;
  case 'c':
    args->color = arg ? arg : "";
    return 0;
  case 'v':
    args->verbose = 1;
    return 0;
  case ARGP_KEY_ARG:
    return 0;
  case ARGP_KEY_END:
    if (!args->output) {
      argp_error(state, "--output is required");
    }
    return 0;
  default:
    return ARGP_ERR_UNKNOWN;
  }
}

static char args_doc[] = "[ARGS...]";
static struct argp argp = {options, parse_opt, args_doc, "mcpcc sample: argp"};

int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    printf("ARG:%s\n", argv[i]);
  }

  struct arguments args = {0};
  argp_parse(&argp, argc, argv, 0, 0, &args);
  return 0;
}
