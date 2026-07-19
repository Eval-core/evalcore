#!/bin/sh
# A stand-in bank support assistant for the quickstart. Reads a customer
# question on stdin and prints a canned, policy-grounded answer — so this
# example runs offline, with no model and no network, yet exercises the real
# scoring path. Swap this target for your live RAG endpoint (openai-compatible
# / http) when you wire it up; the cases, context, and scorers stay identical.
question=$(cat)

case "$question" in
  *refund*|*Refund*|*REFUND*)
    printf '%s' "Your refund is on its way. Approved refunds are processed within 30 business days per policy 4.2, counted from the day the return is approved. If it has been longer than that, reply here and we will escalate."
    ;;
  *fee*|*Fee*|*overdraft*|*charged*)
    printf '%s' "You can dispute the fee. Under policy 7.1 you have 60 calendar days from the statement date to file a dispute, and we reverse the charge while we review it. I have opened the dispute for you."
    ;;
  *lost*|*card*|*stolen*)
    printf '%s' "I have frozen the card so it can not be used. A replacement debit card arrives within 7 to 10 business days per policy 6.4, shipped to the address on file at no charge."
    ;;
  *wire*|*international*|*transfer*)
    printf '%s' "International wire transfers settle in 3 to 5 business days per policy 5.3. You will receive a confirmation with the tracking reference once the funds leave your account."
    ;;
  *)
    printf '%s' "I want to get this right. Per policy 1.4 I am routing you to a specialist who can review your account and follow up within one business day."
    ;;
esac
