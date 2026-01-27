#include "calc.h"
#include "lexer.h"

#include <stddef.h>

// Evaluate expression with + - * / precedence (integer math).
// Grammar:
//   expr   := term ((PLUS|MINUS) term)*
//   term   := factor ((MUL|DIV) factor)*
//   factor := INT
static int parse_factor(Lexer *lx, int *out) {
  Token t = lexer_next(lx);
  if (t.kind != TOK_INT) return 2;
  *out = t.int_value;
  return 0;
}

static int parse_term(Lexer *lx, Token *lookahead, int *out) {
  int acc = 0;
  int rc = parse_factor(lx, &acc);
  if (rc != 0) return rc;

  for (;;) {
    Token op = lexer_next(lx);
    if (op.kind != TOK_MUL && op.kind != TOK_DIV) {
      *lookahead = op;
      break;
    }

    int rhs = 0;
    rc = parse_factor(lx, &rhs);
    if (rc != 0) return rc;

    if (op.kind == TOK_MUL) {
      acc *= rhs;
    } else {
      if (rhs == 0) return 5; // div by zero
      acc /= rhs;
    }
  }

  *out = acc;
  return 0;
}

int calc_eval_int(const char *expr, int *out) {
  if (!expr || !out) return 1;

  Lexer lx;
  lexer_init(&lx, expr);

  Token look = (Token){0};
  int acc = 0;

  // first term
  int term = 0;
  int rc = parse_term(&lx, &look, &term);
  if (rc != 0) return rc;
  acc = term;

  for (;;) {
    Token op = look;
    if (op.kind == TOK_EOF) break;
    if (op.kind != TOK_PLUS && op.kind != TOK_MINUS) return 3;

    rc = parse_term(&lx, &look, &term);
    if (rc != 0) return rc;

    if (op.kind == TOK_PLUS) acc += term;
    else acc -= term;
  }

  *out = acc;
  return 0;
}
