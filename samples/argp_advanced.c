/**
 * argp_advanced.c - Advanced argp sample for mcpcc testing
 *
 * Features tested:
 * - OPTION_ARG_OPTIONAL for optional argument
 * - args_doc for positional argument documentation (INPUT OUTPUT)
 * - Multiple options with doc strings
 */
#include <argp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

const char *argp_program_version = "argp_advanced 1.0";
const char *argp_program_bug_address = "<test@example.com>";

struct arguments {
  const char *input;
  const char *output;
  const char *format;
  const char *compression;
  int verbose;
  int force;
  char **extra_args;
  int extra_count;
};

static struct argp_option options[] = {
    {"verbose", 'v', 0, 0, "Produce verbose output", 0},
    {"format", 'f', "FMT", OPTION_ARG_OPTIONAL,
     "Output format (optional, defaults to 'auto')", 0},
    {"compression", 'c', "LEVEL", OPTION_ARG_OPTIONAL,
     "Compression level (optional, defaults to '6')", 0},
    {"output", 'o', "FILE", 0, "Write output to FILE (required)", 0},
    {"force", 'F', 0, 0, "Force overwrite existing files", 0},
    {0},
};

static error_t parse_opt(int key, char *arg, struct argp_state *state) {
  struct arguments *args = state->input;
  switch (key) {
  case 'v':
    args->verbose = 1;
    return 0;
  case 'f':
    args->format = arg ? arg : "auto";
    return 0;
  case 'c':
    args->compression = arg ? arg : "6";
    return 0;
  case 'o':
    args->output = arg;
    return 0;
  case 'F':
    args->force = 1;
    return 0;
  case ARGP_KEY_ARG:
    // First positional arg is input
    if (!args->input) {
      args->input = arg;
    } else {
      // Collect extra args
      args->extra_args =
          realloc(args->extra_args, (args->extra_count + 1) * sizeof(char *));
      args->extra_args[args->extra_count++] = arg;
    }
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

// args_doc documents the positional arguments
static char args_doc[] = "INPUT [EXTRA...]";

// doc provides a program description
static char doc[] =
    "argp_advanced -- mcpcc sample demonstrating advanced argp features.\v"
    "This program demonstrates OPTION_ARG_OPTIONAL and args_doc usage.";

static struct argp argp = {options, parse_opt, args_doc, doc};

int main(int argc, char **argv) {
  // Print all received arguments for test verification
  for (int i = 1; i < argc; i++) {
    printf("ARG:%s\n", argv[i]);
  }

  struct arguments args = {0};
  argp_parse(&argp, argc, argv, 0, 0, &args);

  // Print parsed values for verification
  if (args.verbose)
    printf("VERBOSE:1\n");
  if (args.format)
    printf("FORMAT:%s\n", args.format);
  if (args.compression)
    printf("COMPRESSION:%s\n", args.compression);
  if (args.output)
    printf("OUTPUT:%s\n", args.output);
  if (args.force)
    printf("FORCE:1\n");
  if (args.input)
    printf("INPUT:%s\n", args.input);

  for (int i = 0; i < args.extra_count; i++) {
    printf("EXTRA:%s\n", args.extra_args[i]);
  }

  free(args.extra_args);
  return 0;
}
