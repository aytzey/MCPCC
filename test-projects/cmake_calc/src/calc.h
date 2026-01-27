#pragma once

// Evaluate an arithmetic expression with + - * /, parentheses, and floats.
// Returns 0 on success; non-zero on parse/runtime error.
int calc_eval_double(const char *expr, double *out);
