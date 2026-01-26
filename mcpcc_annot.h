#pragma once

#if defined(__GNUC__) || defined(__clang__)
  #define MCPCC_SECTION __attribute__((used, section(".mcpcc")))
#else
  #define MCPCC_SECTION
#endif

#define MCPCC_CONCAT_INNER(a, b) a##b
#define MCPCC_CONCAT(a, b) MCPCC_CONCAT_INNER(a, b)

#define MCPCC_TOOL_JSON(json_literal) \
  static const char MCPCC_CONCAT(mcpcc_tool_, __COUNTER__)[] MCPCC_SECTION = "MCPCC_TOOL:" json_literal;

#define MCPCC_PARAM_JSON(json_literal) \
  static const char MCPCC_CONCAT(mcpcc_param_, __COUNTER__)[] MCPCC_SECTION = "MCPCC_PARAM:" json_literal;
