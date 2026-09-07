import pytest

from src.errors import MRTSyntaxError
from src.lexer import Lexer, TokenType


def token_types(source: str):
    return [t.type for t in Lexer(source).scan_tokens()]


def test_single_char_tokens():
    types = token_types("(){}[],;")
    assert types == [
        TokenType.LPAREN, TokenType.RPAREN, TokenType.LBRACE, TokenType.RBRACE,
        TokenType.LBRACKET, TokenType.RBRACKET, TokenType.COMMA, TokenType.SEMICOLON,
        TokenType.EOF,
    ]


def test_multi_char_operators():
    types = token_types("== != <= >= && || !")
    assert types == [
        TokenType.EQUALS, TokenType.NOT_EQUALS, TokenType.LESS_EQUAL,
        TokenType.GREATER_EQUAL, TokenType.AND, TokenType.OR, TokenType.NOT,
        TokenType.EOF,
    ]


def test_modulo_operator():
    types = token_types("10 % 3")
    assert types == [TokenType.NUMBER, TokenType.MODULO, TokenType.NUMBER, TokenType.EOF]


def test_keywords_break_continue():
    types = token_types("break continue")
    assert types == [TokenType.BREAK, TokenType.CONTINUE, TokenType.EOF]


def test_number_literal_is_float():
    tokens = Lexer("42").scan_tokens()
    assert tokens[0].literal == 42.0


def test_string_literal():
    tokens = Lexer('"hello"').scan_tokens()
    assert tokens[0].type == TokenType.STRING
    assert tokens[0].literal == "hello"


def test_string_escape_sequences():
    tokens = Lexer(r'"a\nb\tc\"d"').scan_tokens()
    assert tokens[0].literal == 'a\nb\tc"d'


def test_unterminated_string_raises():
    with pytest.raises(MRTSyntaxError):
        Lexer('"unterminated').scan_tokens()


def test_line_and_block_comments_are_skipped():
    types = token_types("1 // comment\n2 /* block \n comment */ 3")
    assert types == [TokenType.NUMBER, TokenType.NUMBER, TokenType.NUMBER, TokenType.EOF]


def test_unterminated_block_comment_raises():
    with pytest.raises(MRTSyntaxError):
        Lexer("/* never closed").scan_tokens()


def test_lone_ampersand_raises():
    with pytest.raises(MRTSyntaxError):
        Lexer("&").scan_tokens()


def test_lone_pipe_raises():
    with pytest.raises(MRTSyntaxError):
        Lexer("|").scan_tokens()


def test_unexpected_character_raises():
    with pytest.raises(MRTSyntaxError):
        Lexer("@").scan_tokens()
