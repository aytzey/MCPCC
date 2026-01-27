#pragma once

typedef enum {
  TOK_INT,
  TOK_PLUS,
  TOK_MINUS,
  TOK_EOF,
  TOK_ERR,
} TokenKind;

typedef struct {
  TokenKind kind;
  int int_value;
} Token;

typedef struct {
  const char *p;
} Lexer;

void lexer_init(Lexer *lx, const char *input);
Token lexer_next(Lexer *lx);
