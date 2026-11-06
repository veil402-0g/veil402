"""A Langflow component: ask another agent to build a smart contract and pay
for it privately over x402.

Drop it in a flow as  Chat Input -> Commission Contract -> Chat Output.
You type what you need; it gets a 402 from the ContractShop, pays through your
Veil wallet (the chain shows only a commitment), waits for the build, and
returns the contract. Everything after your one message happens on its own.
"""

import json
import time
import urllib.error
import urllib.request

from langflow.custom import Component
from langflow.io import MessageTextInput, Output, StrInput
from langflow.schema.message import Message


class CommissionContractComponent(Component):
    display_name = "Commission Contract"
    description = "Commission a smart contract from the ContractShop agent and pay for it privately over x402."
    icon = "file-signature"
    name = "CommissionContract"

    inputs = [
        MessageTextInput(
            name="request",
            display_name="What to build",
            info="Plain-language description of the contract you need.",
            required=True,
        ),
        StrInput(name="shop", display_name="ContractShop URL", value="http://127.0.0.1:8500", advanced=True),
        StrInput(name="daemon", display_name="Veil daemon API", value="http://127.0.0.1:8402/api/v1", advanced=True),
    ]
    outputs = [Output(name="contract", display_name="Contract", method="commission")]

    def _post(self, url: str, body: dict):
        data = json.dumps(body).encode()
        req = urllib.request.Request(
            url, data=data, headers={"Content-Type": "application/json"}, method="POST"
        )
        try:
            with urllib.request.urlopen(req, timeout=900) as resp:
                return resp.status, json.loads(resp.read().decode())
        except urllib.error.HTTPError as exc:
            try:
                return exc.code, json.loads(exc.read().decode())
            except Exception:
                return exc.code, {}

    def commission(self) -> Message:
        request = self.request
        if not isinstance(request, str):
            request = getattr(request, "text", str(request))

        self.status = "asking the ContractShop…"
        code, invoice = self._post(f"{self.shop}/task", {"request": request})
        if code != 402:
            return Message(text=f"the shop did not ask for payment (got {code})")
        price, pay_to, invoice_id = invoice["price"], invoice["payTo"], invoice["invoiceId"]

        self.status = f"402 — paying {price / 1_000_000:.2f} USDC privately…"
        code, pay = self._post(f"{self.daemon}/pay", {"address": pay_to, "amount": price})
        if pay.get("status") != "ok":
            return Message(text=f"payment failed: {pay.get('error')}")
        proof = pay["result"]

        self.status = "paid; waiting for the build…"
        for _ in range(200):
            code, out = self._post(
                f"{self.shop}/task",
                {
                    "invoiceId": invoice_id,
                    "commitment": proof["recipient_commitment"],
                    "blinding": proof["blinding"],
                },
            )
            if code == 200:
                contract = out.get("contract") or "a contract"
                head = f"### ✅ Delivered **{contract}**"
                if out.get("address"):
                    head += f"\n`{out['address']}`"
                lines = [head, ""]
                if out.get("network"):
                    lines.append(f"- **network:** {out['network']}")
                if out.get("explorerUrl"):
                    lines.append(f"- **explorer:** {out['explorerUrl']}")
                lines.append(
                    f"- 💸 **paid {price / 1_000_000:.2f} USDC privately** — on chain this is only a "
                    f"commitment: *no sender, no recipient, no amount*"
                )
                if out.get("source"):
                    lines += ["", "```solidity", out["source"].rstrip(), "```"]
                self.status = "done"
                return Message(text="\n".join(lines))
            time.sleep(2)
        return Message(text="the shop never confirmed the payment")
