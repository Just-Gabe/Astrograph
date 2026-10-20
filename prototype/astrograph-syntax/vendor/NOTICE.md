# Gramáticas compiladas aqui

`luap/parser.c` e `luadoc/parser.c` (com `tree_sitter/*.h`) são os parsers gerados
dos crates `tree-sitter-luap` 1.0.0 (https://github.com/amaanq/tree-sitter-luap)
e `tree-sitter-luadoc` 1.1.0 (https://github.com/amaanq/tree-sitter-luadoc),
licença MIT (autoria e texto da licença nos repositórios). Os crates dependem do `tree-sitter` 0.20 (API
antiga); compilar só o `parser.c` evita uma segunda cópia da biblioteca C no executável.
