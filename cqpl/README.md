### CQPL

## Notes:

- dividere i concetti di taint souce e taint sink, così non è ambiguo il fatto che si debba matchare la condizione sullo statement o sul path 
    - piu condizioni da matchare su stmnt src o snk
    - posso avere + sources o sinks? Direi di sì, altrimenti come faccio a definire le DF e UAF? 
    - con lts avrei il concetto di uno statement dopo l'altro, ma con il mio icfg?? Adesso vedo


```bash
- <patter-to-find>.cqpl
```
```bash
### file.cqpl:
# rule                      // name of the rule 
***                         // variables
taint_src: <predicate()>    // can avoid to specifiy within the () variables names if ***
taint_snk: <predicate()>    // same
```
### anche predicati su snk
- solo un snk:                          taint_snk: ...

- 1 o più snk statement:                *@taint_snk: ...  

- più snk specifici:                    taint_snk: ...
                                        taint_snk: ...
                                        taint_snk: ...

- più src ad un snk:                    *@taint_src: ...
                                        taint_snk: ...
- caso analogo con src specifici ....