#!/bin/sh
# A stand-in bank support assistant. Reads a customer question on stdin and
# prints a canned, policy-grounded answer — so this example runs offline, with
# no model and no network, yet exercises the real scoring path. Swap this
# target for your live RAG endpoint (openai-compatible / http) when you wire it
# up; the cases, context, and scorers stay exactly the same.
question=$(cat)

case "$question" in
  *refund*|*Refund*|*REFUND*)
    printf '%s' "Your refund is on its way. Refunds are processed within 30 business days per policy 4.2, counted from the day the return is approved. If it has been longer than that, reply here and we will escalate."
    ;;
  *overdraft*|*fee*|*Fee*|*charged*)
    printf '%s' "You can dispute the fee. Under policy 7.1 you have 60 calendar days from the statement date to file a fee dispute, and we reverse the charge while we review it. I have opened the dispute for you."
    ;;
  *balance*|*Balance*)
    printf '%s' "For your security I can not share account balances over chat. Per policy 2.1 you will need to verify your identity in the mobile app or at a branch before any account details can be released."
    ;;
  *wire*|*international*|*transfer*)
    printf '%s' "International wire transfers settle in 3 to 5 business days per policy 5.3. You will receive a confirmation with the tracking reference once the funds leave your account."
    ;;
  *lost*|*card*|*stolen*)
    printf '%s' "I have frozen the card so it can not be used. A replacement debit card arrives within 7 to 10 business days per policy 6.4, shipped to the address on file at no charge."
    ;;
  *dispute*|*transaction*|*unauthorized*)
    printf '%s' "Your transaction dispute is under review. Per policy 7.2 we issue provisional credit within 10 business days while we investigate, and a final decision follows within 45 days."
    ;;
  *statement*|*copy*)
    printf '%s' "You can download the last 24 months of statements in online banking under Documents. Per policy 8.1 a mailed paper copy is also available and arrives within 5 business days on request."
    ;;
  *)
    printf '%s' "I want to get this right. Per policy 1.4 I am routing you to a specialist who can review your account and follow up within one business day."
    ;;
esac
