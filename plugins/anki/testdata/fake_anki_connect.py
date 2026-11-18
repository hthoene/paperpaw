#!/usr/bin/env python3
"""A fake AnkiConnect (classic API, version 6) for trying the plugin on a
device or in a shell without Anki: a small collection in memory, the same
request and reply shapes as the add-on's documented API.

    python3 fake_anki_connect.py [--port 8765] [--bind 0.0.0.0] [--key SECRET]

Actions: deckNames, findCards, cardsInfo, answerCards. Every answer taken is
printed, and a card answered leaves the due cards. Nothing of Anki is used.
"""
import argparse
import json
import re
from http.server import BaseHTTPRequestHandler, HTTPServer

CARDS = [
    # id, deck, front, back, new, due
    (1001, "Spanish", "el perro", "the dog", False, True),
    (1002, "Spanish", "el gato", "the cat", False, True),
    (1003, "Spanish::Verbs", "ser", "to be", True, False),
    (1004, "Spanish::Verbs", "estar", "to be (state)", True, False),
    (2001, "Deutsch", "der Hund", "<b>dog</b><br>(Tier)", False, True),
]
STATE = {c[0]: {"due": c[5], "new": c[4]} for c in CARDS}


def deck_matches(card_deck, wanted):
    return wanted is None or card_deck == wanted or card_deck.startswith(wanted + "::")


def handle(request, key):
    if key and request.get("key") != key:
        raise ValueError("valid api key must be provided")
    action, params = request.get("action"), request.get("params") or {}
    if action == "deckNames":
        return sorted({c[1] for c in CARDS})
    if action == "findCards":
        query = params.get("query", "")
        deck = re.search(r'deck:"((?:[^"\\]|\\.)*)"', query)
        deck = re.sub(r"\\(.)", r"\1", deck.group(1)) if deck else None
        due, new = "is:due" in query, "is:new" in query
        return [
            c[0]
            for c in CARDS
            if deck_matches(c[1], deck)
            and ((due and STATE[c[0]]["due"]) or (new and STATE[c[0]]["new"]))
        ]
    if action == "cardsInfo":
        out = []
        for cid in params.get("cards", []):
            card = next((c for c in CARDS if c[0] == cid), None)
            if card is None:
                out.append({})
                continue
            style = "<style>.card { font-family: arial; }</style>"
            out.append({
                "cardId": cid,
                "fields": {"Front": {"value": card[2], "order": 0}},
                "question": style + card[2],
                "answer": style + card[2] + "<hr id=answer>" + card[3],
                "deckName": card[1],
                "css": ".card { font-family: arial; }",
            })
        return out
    if action == "answerCards":
        results = []
        for answer in params.get("answers", []):
            known = answer["cardId"] in STATE
            if known:
                STATE[answer["cardId"]] = {"due": False, "new": False}
                print(f"answered {answer['cardId']} ease {answer['ease']}", flush=True)
            results.append(known)
        return results
    raise ValueError(f"unsupported action: {action}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--bind", default="0.0.0.0")
    parser.add_argument("--key", default=None)
    args = parser.parse_args()

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            try:
                reply = {"result": handle(json.loads(body), args.key), "error": None}
            except Exception as error:  # the add-on answers an error as text
                reply = {"result": None, "error": str(error)}
            data = json.dumps(reply).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *args):
            pass

    print(f"fake AnkiConnect on {args.bind}:{args.port}", flush=True)
    HTTPServer((args.bind, args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
