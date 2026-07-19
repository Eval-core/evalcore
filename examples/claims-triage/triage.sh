#!/bin/sh
# A stand-in insurance claims classifier. Reads a free-text claim description on
# stdin and routes it to exactly one of four labels: auto, property, injury, or
# fraud-review. It is deliberately naive keyword routing — good enough to be
# realistic, and it misclassifies one case on purpose (a staged claim whose
# "collision" wording pulls it into `auto` before the fraud signal is ever
# checked) so the report is interesting while the gates still pass. Swap this
# for your real classifier (an openai-compatible or http endpoint); the cases,
# labels, and gates below do not change.
claim=$(cat)
lc=$(printf '%s' "$claim" | tr 'A-Z' 'a-z')

# Order matters: a vehicle keyword wins before the fraud signal is reached.
if printf '%s' "$lc" | grep -qE 'car|vehicle|truck|bumper|fender|windshield|collision|rear-ended'; then
  printf '%s' auto
elif printf '%s' "$lc" | grep -qE 'pipe|flood|basement|fire|smoke|roof|drywall'; then
  printf '%s' property
elif printf '%s' "$lc" | grep -qE 'injur|fractured|whiplash|hospital|slipped|therapy'; then
  printf '%s' injury
elif printf '%s' "$lc" | grep -qE 'duplicate|staged|suspicious|inconsistent|fabricat'; then
  printf '%s' fraud-review
else
  printf '%s' fraud-review
fi
