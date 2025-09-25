### CQPL

## Notes:

- dividere i concetti di taint souce e taint sink, così non è ambiguo il fatto che si debba matchare la condizione sullo statement o sul path 
    - piu condizioni da matchare su stmnt src o snk
    - posso avere + sources o sinks? Direi di sì, altrimenti come faccio a definire le DF e UAF? 
    - con lts avrei il concetto di uno statement dopo l'altro, ma con il mio icfg?? Adesso vedo


```bash
- <patter-to-find>.cqpl
```
can have more tain_src || taint_snk

Pattern P ::= 
# RULE  
## DOMAIN
v: \\variables 
[S]:
    taint_src: ...
    taint_snk: ...
Variables v ::= |tt |q| x || q t ∗ || ∗ t ∗|| ∗ ∗ ∗ || q ∗ ∗
Types T ::= isize | usize | . . . | array | tuple | vec | function | trait | ref | enum | struct | option | box 
Statement S ::= p(x) where p is a predicate
| S && S | S || S | !S
| ∗ (wildcard: zero o pi`u statements qualsiasi)
| |> a order between multiple sources || sinks
Expression E ::= v | c | E binop E | unop E | f (E1, . . . , En)
Qualifier q ::= imm | mut
Constants c ::= literals: 0, 1, true, "str", . . .
Predicate: alloc(v) | drop(v) | use(v) | read(v) | write(v) | assign(v)
where alloc is an heap allocatio operation like box new. drop any operation freeing the memory object, use can be either read || write.
can specify in the predicate () the specifi variable or taken from the field v from the rule



```bash
### file.cqpl:
# rule                          // name of the rule 
## domanin: memory || general   // which abstract domain
v: **|*|*                          // type type| qualifier | variable name. For example ** can be Box::int or Box::* for any box type. qualifier: mutable or imm or all. specific var name or all variable name
domain:
taint_src: <predicate()>        // can avoid to specifiy within the () variables names if ***
taint_snk: <predicate()>        // same
```
at the and of a predicate of a sink I can say that the first snk specified must be first in the cfg path wrt the second specified with the operator |>.
for example: 
# uaf
**|*|*
taint_src: *@ alloc
taint_snk: drop |> 
taint_snk: use
---

con *@ posso dire 1 o piu snk o src. ad esempio: ho una double free se ho uno stmnt src che allochi heap(alloc is box new) e piu di unsnk con una drop() su ogni variabile:
# double_free
***
taint_src: alloc()
@taint_snk: *@ drop()




### anche predicati su snk
- solo un snk:                          taint_snk: ...

- 1 o più snk statement:                *@taint_snk: ...  

- più snk specifici:                    taint_snk: ...
                                        taint_snk: ...
                                        taint_snk: ...

- più src ad un snk:                    *@taint_src: ...
                                        taint_snk: ...
- caso analogo con src specifici ....