#include <stdio.h>

#include "../mcpcc_annot.h"

MCPCC_TOOL_JSON("{\"name\":\"annotated\",\"title\":\"Annotated Sample\",\"description\":\"mcpcc sample tool via annotations\"}");
MCPCC_PARAM_JSON("{\"tool\":\"annotated\",\"property\":\"verbose\",\"long\":\"--verbose\",\"short\":\"-v\",\"type\":\"boolean\",\"description\":\"More logs\"}");
MCPCC_PARAM_JSON("{\"tool\":\"annotated\",\"property\":\"output\",\"long\":\"--output\",\"short\":\"-o\",\"type\":\"string\",\"takesValue\":true,\"required\":true,\"description\":\"Output file\"}");

int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    printf("ARG:%s\n", argv[i]);
  }
  return 0;
}

