# Chapter 4.1: Node Architecture and Event Loop

The node event loop receives P2P messages, validates them, forwards data to `ChainActor`, and publishes follow-up requests.

For PQ-QC, a node requests `GetQcBlob` when a finality certificate is missing its sidecar. The blockchain keeps the certificate pending, so importing the blob later can complete finality without waiting for the cert to be rebroadcast.

