#include "lexer.h"

#include <ctype.h>
#include <stdlib.h>

static void skip_ws(Lexer *lx) {
  while (*lx->p && isspace((unsigned char)*lx->p)) lx->p++;
}

void lexer_init(Lexer *lx, const char *input) { lx->p = input ? input : ""; }

static Token tok(TokenKind k) {
  Token t;
  t.kind = k;
  t.num_value = 0.0;
  return t;
}

Token lexer_next(Lexer *lx) {
  skip_ws(lx);
  char c = *lx->p;
  if (!c) return tok(TOK_EOF);

  switch (c) {
    case '+':
      lx->p++;
      return tok(TOK_PLUS);
    case '-':
      lx->p++;
      return tok(TOK_MINUS);
    case '*':
      lx->p++;
      return tok(TOK_MUL);
    case '/':
      lx->p++;
      return tok(TOK_DIV);
    case '(':
      lx->p++;
      return tok(TOK_LPAREN);
    case ')':
      lx->p++;
      return tok(TOK_RPAREN);
    default:
      break;
  }

  // Number: use strtod so we support floats like 1.23, .5, 2.
  if (isdigit((unsigned char)c) || c == '.') {
    char *endp = NULL;
    double v = strtod(lx->p, &endp);
    if (endp == lx->p) {
      // '.' not followed by digits, etc.
      lx->p++;
      return tok(TOK_ERR);
    }
    lx->p = endp;
    Token t = tok(TOK_NUM);
    t.num_value = v;
    return t;
  }

  // Unknown char
  lx->p++;
  return tok(TOK_ERR);
}
