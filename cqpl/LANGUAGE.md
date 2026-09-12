# CQPL v1 — sintassi implementata

## Formule di stato

```text
phi ::= p(x)
      | q(x)
      | !phi
      | phi && phi
      | phi || phi
      | exists x. phi
      | forall x. phi
      | E psi
      | A psi
```

## Formule di cammino

```text
psi ::= X phi
      | F phi
      | G phi
      | phi U phi
      | phi
```

Nella sintassi concreta sono disponibili le abbreviazioni CTL:

```text
EX AX EF AF EG AG
E[phi U psi]
A[phi U psi]
```

## Predicati may

```text
alloc(x)
drop(x)
own_forg(x)
```

Un test positivo produce `unk`, non `tt`.

## Predicati label

```text
alloc_l(x)
drop_l(x)
read_l(x)
write_l(x)
use_l(x)
```

Producono `tt` o `ff`.

## Precedenza

Da più forte a più debole:

```text
! / quantificatori / operatori temporali unari
&&
||
```

Usare parentesi quando la portata non è ovvia.
