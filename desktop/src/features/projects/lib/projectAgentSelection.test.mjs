import assert from "node:assert/strict";
import test from "node:test";

import { pickDefaultProjectsAgent } from "./projectAgentSelection.ts";

test("prefers Diego over the first running agent", () => {
  const implementationPartner = {
    name: "Implementation Partner",
    personaId: "custom:implementation",
  };
  const fizz = { name: "Diego", personaId: "builtin:diego" };
  assert.equal(pickDefaultProjectsAgent([implementationPartner, fizz]), fizz);
});

test("ignores an unmanaged agent using the Diego display name", () => {
  const managed = { name: "Builder", personaId: "custom:builder" };
  const spoofedDiego = { name: "Diego" };
  assert.equal(pickDefaultProjectsAgent([managed, spoofedDiego]), managed);
  assert.equal(pickDefaultProjectsAgent([managed]), managed);
  assert.equal(pickDefaultProjectsAgent([]), null);
});
