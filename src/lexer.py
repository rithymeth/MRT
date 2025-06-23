from enum import Enum, auto
from dataclasses import dataclass
from typing import Optional, List

class TokenType(Enum):
    # Keywords
    FUNC = auto()
    RETURN = auto()
    IF = auto()
    ELSE = auto()
    WHILE = auto()
    FOR = auto()
    PRINT = auto()
    VAR = auto()
    TRUE = auto()
    FALSE = auto()
    
    # Literals
    IDENTIFIER = auto()
    NUMBER = auto()
    STRING = auto()
    
    # Operators
    PLUS = auto()
    MINUS = auto()
    MULTIPLY = auto()
    DIVIDE = auto()
    ASSIGN = auto()
    EQUALS = auto()
    NOT_EQUALS = auto()
    GREATER = auto()
    GREATER_EQUAL = auto()
    LESS = auto()
    LESS_EQUAL = auto()
    
    # Delimiters
    LPAREN = auto()
    RPAREN = auto()
    LBRACE = auto()
    RBRACE = auto()
    LBRACKET = auto()
    RBRACKET = auto()
    COMMA = auto()
    SEMICOLON = auto()
    
    # Special
    EOF = auto()

@dataclass
class Token:
    type: TokenType
    lexeme: str
    literal: Optional[object]
    line: int

class Lexer:
    def __init__(self, source: str):
        self.source = source
        self.tokens: List[Token] = []
        self.start = 0
        self.current = 0
        self.line = 1
        
        self.keywords = {
            "func": TokenType.FUNC,
            "return": TokenType.RETURN,
            "if": TokenType.IF,
            "else": TokenType.ELSE,
            "while": TokenType.WHILE,
            "for": TokenType.FOR,
            "print": TokenType.PRINT,
            "var": TokenType.VAR,
            "true": TokenType.TRUE,
            "false": TokenType.FALSE,
        }
    
    def scan_tokens(self) -> List[Token]:
        while not self.is_at_end():
            self.start = self.current
            self.scan_token()
            
        self.tokens.append(Token(TokenType.EOF, "", None, self.line))
        return self.tokens
    
    def scan_token(self):
        c = self.advance()
        match c:
            case '(': self.add_token(TokenType.LPAREN)
            case ')': self.add_token(TokenType.RPAREN)
            case '{': self.add_token(TokenType.LBRACE)
            case '}': self.add_token(TokenType.RBRACE)
            case '[': self.add_token(TokenType.LBRACKET)
            case ']': self.add_token(TokenType.RBRACKET)
            case ',': self.add_token(TokenType.COMMA)
            case ';': self.add_token(TokenType.SEMICOLON)
            case '+': self.add_token(TokenType.PLUS)
            case '-': self.add_token(TokenType.MINUS)
            case '*': self.add_token(TokenType.MULTIPLY)
            case '/':
                if self.match('/'):
                    # Comment goes until end of line
                    while self.peek() != '\n' and not self.is_at_end():
                        self.advance()
                elif self.match('*'):
                    # Multi-line comment
                    self.block_comment()
                else:
                    self.add_token(TokenType.DIVIDE)
            case ' ' | '\r' | '\t': pass  # Ignore whitespace
            case '\n': self.line += 1
            case '"': self.string()
            case '>': 
                if self.match('='):
                    self.add_token(TokenType.GREATER_EQUAL)
                else:
                    self.add_token(TokenType.GREATER)
            case '<':
                if self.match('='):
                    self.add_token(TokenType.LESS_EQUAL)
                else:
                    self.add_token(TokenType.LESS)
            case '=':
                if self.match('='):
                    self.add_token(TokenType.EQUALS)
                else:
                    self.add_token(TokenType.ASSIGN)
            case '!':
                if self.match('='):
                    self.add_token(TokenType.NOT_EQUALS)
                else:
                    raise Exception(f"Unexpected character '!' at line {self.line}")
            case _:
                if self.is_digit(c):
                    self.number()
                elif self.is_alpha(c):
                    self.identifier()
                else:
                    raise Exception(f"Unexpected character '{c}' at line {self.line}")
    
    def block_comment(self):
        """Handle multi-line comments /* ... */"""
        while not self.is_at_end():
            if self.peek() == '*' and self.peek_next() == '/':
                # Consume the closing */
                self.advance()  # consume *
                self.advance()  # consume /
                return
            if self.peek() == '\n':
                self.line += 1
            self.advance()
        
        # If we reach here, the comment was not closed
        raise Exception(f"Unterminated comment starting at line {self.line}")
    
    def identifier(self):
        while self.is_alphanumeric(self.peek()):
            self.advance()
            
        text = self.source[self.start:self.current]
        token_type = self.keywords.get(text, TokenType.IDENTIFIER)
        
        # Handle boolean literals
        if token_type == TokenType.TRUE:
            self.add_token(token_type, True)
        elif token_type == TokenType.FALSE:
            self.add_token(token_type, False)
        else:
            self.add_token(token_type)
    
    def number(self):
        while self.is_digit(self.peek()):
            self.advance()
            
        # Look for decimal point
        if self.peek() == '.' and self.is_digit(self.peek_next()):
            self.advance()  # Consume the "."
            while self.is_digit(self.peek()):
                self.advance()
                
        value = float(self.source[self.start:self.current])
        self.add_token(TokenType.NUMBER, value)
    
    def string(self):
        # Find the closing quote
        start_line = self.line
        while self.peek() != '"' and not self.is_at_end():
            if self.peek() == '\n':
                self.line += 1
            self.advance()
        
        if self.is_at_end():
            raise Exception(f"Unterminated string starting at line {start_line}")
        
        # Skip the closing quote
        self.advance()
        
        # Get the string value (without quotes)
        value = self.source[self.start + 1:self.current - 1]
        self.add_token(TokenType.STRING, value)
    
    def match(self, expected: str) -> bool:
        if self.is_at_end():
            return False
        if self.source[self.current] != expected:
            return False
            
        self.current += 1
        return True
    
    def peek(self) -> str:
        if self.is_at_end():
            return '\0'
        return self.source[self.current]
    
    def peek_next(self) -> str:
        if self.current + 1 >= len(self.source):
            return '\0'
        return self.source[self.current + 1]
    
    def is_alpha(self, c: str) -> bool:
        return ('a' <= c <= 'z') or ('A' <= c <= 'Z') or c == '_'
    
    def is_digit(self, c: str) -> bool:
        return '0' <= c <= '9'
    
    def is_alphanumeric(self, c: str) -> bool:
        return self.is_alpha(c) or self.is_digit(c)
    
    def is_at_end(self) -> bool:
        return self.current >= len(self.source)
    
    def advance(self) -> str:
        self.current += 1
        return self.source[self.current - 1]
    
    def add_token(self, type: TokenType, literal: Optional[object] = None):
        text = self.source[self.start:self.current]
        self.tokens.append(Token(type, text, literal, self.line))