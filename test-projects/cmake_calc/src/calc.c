#include "calc.h"
#include "lexer.h"

#include <stddef.h>

// Recursive descent parser with precedence and parentheses.
// Grammar:
//   expr    := term ((PLUS|MINUS) term)*
//   term    := unary ((MUL|DIV) unary)*
//   unary   := (PLUS|MINUS)* primary
//   primary := NUM | '(' expr ')'

typedef struct {
  Lexer lx;
  Token look;
} Parser;

static void parser_init(Parser *p, const char *input) {
  lexer_init(&p->lx, input);
  p->look = lexer_next(&p->lx);
}

static void consume(Parser *p) { p->look = lexer_next(&p->lx); }

static int parse_expr(Parser *p, double *out); // fwd

static int parse_primary(Parser *p, double *out) {
  if (p->look.kind == TOK_NUM) {
    *out = p->look.num_value;
    consume(p);
    return 0;
  }

  if (p->look.kind == TOK_LPAREN) {
    consume(p);
    int rc = parse_expr(p, out);
    if (rc != 0) return rc;
    if (p->look.kind != TOK_RPAREN) return 6; // missing ')'
    consume(p);
    return 0;
  }

  return 2; // expected primary
}

static int parse_unary(Parser *p, double *out) {
  int sign = 1;
  while (p->look.kind == TOK_PLUS || p->look.kind == TOK_MINUS) {
    if (p->look.kind == TOK_MINUS) sign = -sign;
    consume(p);
  }

  double v = 0.0;
  int rc = parse_primary(p, &v);
  if (rc != 0) return rc;
  *out = sign * v;
  return 0;
}

static int parse_term(Parser *p, double *out) {
  double acc = 0.0;
  int rc = parse_unary(p, &acc);
  if (rc != 0) return rc;

  for (;;) {
    TokenKind k = p->look.kind;
    if (k != TOK_MUL && k != TOK_DIV) break;
    consume(p);

    double rhs = 0.0;
    rc = parse_unary(p, &rhs);
    if (rc != 0) return rc;

    if (k == TOK_MUL) {
      acc *= rhs;
    } else {
      if (rhs == 0.0) return 5; // div by zero
      acc /= rhs;
    }
  }

  *out = acc;
  return 0;
}

static int parse_expr(Parser *p, double *out) {
  double acc = 0.0;
  int rc = parse_term(p, &acc);
  if (rc != 0) return rc;

  for (;;) {
    TokenKind k = p->look.kind;
    if (k != TOK_PLUS && k != TOK_MINUS) break;
    consume(p);

    double rhs = 0.0;
    rc = parse_term(p, &rhs);
    if (rc != 0) return rc;

    if (k == TOK_PLUS) acc += rhs;
    else acc -= rhs;
  }

  *out = acc;
  return 0;
}

int calc_eval_double(const char *expr, double *out) {
  if (!expr || !out) return 1;

  Parser p;
  parser_init(&p, expr);

  double v = 0.0;
  int rc = parse_expr(&p, &v);
  if (rc != 0) return rc;

  if (p.look.kind != TOK_EOF) {
    // trailing junk
    return 7;
  }

  *out = v;
  return 0;
}
