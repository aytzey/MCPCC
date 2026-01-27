#pragma once

typedef enum {
  TOK_NUM,
  TOK_PLUS,
  TOK_MINUS,
  TOK_MUL,
  TOK_DIV,
  TOK_LPAREN,
  TOK_RPAREN,
  TOK_EOF,
  TOK_ERR,
} TokenKind;

typedef struct {
  TokenKind kind;
  double num_value;
} Token;

typedef struct {
  const char *p;
} Lexer;

void lexer_init(Lexer *lx, const char *input);
Token lexer_next(Lexer *lx);
