#include "lexer.h"

#include <ctype.h>

static void skip_ws(Lexer *lx) {
  while (*lx->p && isspace((unsigned char)*lx->p)) lx->p++;
}

void lexer_init(Lexer *lx, const char *input) {
  lx->p = input ? input : "";
}

Token lexer_next(Lexer *lx) {
  skip_ws(lx);
  Token t = {0};
  char c = *lx->p;
  if (!c) {
    t.kind = TOK_EOF;
    return t;
  }

  if (c == '+') {
    lx->p++;
    t.kind = TOK_PLUS;
    return t;
  }
  if (c == '-') {
    lx->p++;
    t.kind = TOK_MINUS;
    return t;
  }

  if (isdigit((unsigned char)c)) {
    int v = 0;
    while (isdigit((unsigned char)*lx->p)) {
      v = v * 10 + (*lx->p - '0');
      lx->p++;
    }
    t.kind = TOK_INT;
    t.int_value = v;
    return t;
  }

  // Unknown char
  lx->p++;
  t.kind = TOK_ERR;
  return t;
}
