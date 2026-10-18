# Telemetrie serie commune

## Format

Le format commun est `JSON Lines`, une ligne JSON par evenement.

Pourquoi:

- simple a produire cote firmware
- simple a parser cote PC
- compatible replay et export
- robuste face au streaming serie

## Envelope minimale

Champs communs:

- `ts`: timestamp ISO-8601 ou compteur local si l'horloge embarquee n'est pas disponible
- `node`: identifiant logique du noeud (`stm32_a`, `stm32_b`, `arduino_a`, `arduino_b`)
- `type`: type d'evenement
- `meta`: dictionnaire libre de metadonnees

## Types supportes

- `heartbeat`
- `can_tx`
- `can_rx`
- `error`
- `state`
- `measure`
- `debug`

## Exemple

```json
{
  "ts": "2026-03-07T10:15:30.125Z",
  "node": "stm32_a",
  "type": "can_tx",
  "id": "0x101",
  "dlc": 8,
  "data": [1, 2, 3, 4, 5, 6, 7, 8],
  "meta": {
    "bitrate": 500000,
    "channel": "can1"
  }
}
```

## Recommandations firmware

- toujours terminer par `\n`
- eviter les payloads trop longs sur UART
- envoyer `heartbeat` toutes les 1 s
- reserver `debug` aux infos humaines utiles
- structurer `error` avec `code`, `message` et `severity`

