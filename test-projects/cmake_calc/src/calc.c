#include "calc.h"
#include "lexer.h"

#include <stddef.h>

int calc_eval_int(const char *expr, int *out) {
  if (!expr || !out) return 1;

  Lexer lx;
  lexer_init(&lx, expr);

  // Grammar: int (('+'|'-') int)*
  Token t = lexer_next(&lx);
  if (t.kind != TOK_INT) return 2;

  int acc = t.int_value;

  for (;;) {
    Token op = lexer_next(&lx);
    if (op.kind == TOK_EOF) break;
    if (op.kind != TOK_PLUS && op.kind != TOK_MINUS) return 3;

    Token rhs = lexer_next(&lx);
    if (rhs.kind != TOK_INT) return 4;

    if (op.kind == TOK_PLUS) acc += rhs.int_value;
    else acc -= rhs.int_value;
  }

  *out = acc;
  return 0;
}
