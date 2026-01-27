/**
 * annotated_advanced.c - Advanced annotation sample for mcpcc testing
 *
 * Features tested:
 * - Tool annotation with custom name, title, description
 * - Param annotations with type, required, description overrides
 * - Combination of annotation with heuristic extraction
 */
#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>

#include "../mcpcc_annot.h"

// Tool annotation overrides the default tool name and provides descriptions
MCPCC_TOOL_JSON("{\"name\":\"file-processor\",\"title\":\"File Processor "
                "Tool\",\"description\":\"Process files with configurable "
                "options. Supports multiple output formats.\"}");

// Param annotations provide detailed descriptions and type information
MCPCC_PARAM_JSON("{\"tool\":\"file-processor\",\"property\":\"input\",\"long\":"
                 "\"--input\",\"short\":\"-i\",\"type\":\"string\",\"required\":"
                 "true,\"description\":\"Input file path to process\"}");

MCPCC_PARAM_JSON(
    "{\"tool\":\"file-processor\",\"property\":\"output\",\"long\":\"--output\","
    "\"short\":\"-o\",\"type\":\"string\",\"required\":true,\"description\":"
    "\"Output file path for results\"}");

MCPCC_PARAM_JSON(
    "{\"tool\":\"file-processor\",\"property\":\"verbose\",\"long\":\"--"
    "verbose\",\"short\":\"-v\",\"type\":\"boolean\",\"description\":\"Enable "
    "verbose logging output\"}");

MCPCC_PARAM_JSON("{\"tool\":\"file-processor\",\"property\":\"threads\","
                 "\"long\":\"--threads\",\"short\":\"-t\",\"type\":\"integer\","
                 "\"description\":\"Number of processing threads (default: 1)"
                 "\"}");

MCPCC_PARAM_JSON(
    "{\"tool\":\"file-processor\",\"property\":\"format\",\"long\":\"--format\","
    "\"short\":\"-f\",\"type\":\"string\",\"description\":\"Output format "
    "(json, csv, xml)\"}");

// The program also has getopt_long for actual CLI parsing
// mcpcc should merge annotations with extracted options

int main(int argc, char **argv) {
  // Print all received arguments for test verification
  for (int i = 1; i < argc; i++) {
    printf("ARG:%s\n", argv[i]);
  }

  int opt = 0;
  int verbose = 0;
  int threads = 1;
  const char *input = NULL;
  const char *output = NULL;
  const char *format = NULL;

  static struct option long_options[] = {
      {"input", required_argument, 0, 'i'},
      {"output", required_argument, 0, 'o'},
      {"verbose", no_argument, 0, 'v'},
      {"threads", required_argument, 0, 't'},
      {"format", required_argument, 0, 'f'},
      {0, 0, 0, 0},
  };

  while ((opt = getopt_long(argc, argv, "i:o:vt:f:", long_options, 0)) != -1) {
    switch (opt) {
    case 'i':
      input = optarg;
      break;
    case 'o':
      output = optarg;
      break;
    case 'v':
      verbose = 1;
      break;
    case 't':
      threads = atoi(optarg);
      break;
    case 'f':
      format = optarg;
      break;
    default:
      fprintf(stderr,
              "Usage: %s -i INPUT -o OUTPUT [-v] [-t THREADS] [-f FORMAT]\n",
              argv[0]);
      return 2;
    }
  }

  if (!input || !output) {
    fprintf(stderr, "missing --input or --output\n");
    return 2;
  }

  // Print parsed values for verification
  printf("INPUT:%s\n", input);
  printf("OUTPUT:%s\n", output);
  if (verbose)
    printf("VERBOSE:1\n");
  printf("THREADS:%d\n", threads);
  if (format)
    printf("FORMAT:%s\n", format);

  // Print remaining positional arguments
  for (int i = optind; i < argc; i++) {
    printf("POSITIONAL:%s\n", argv[i]);
  }

  return 0;
}
