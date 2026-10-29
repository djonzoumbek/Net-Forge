/** Number of lines a paste would send. A single trailing newline does not count
 * as an extra line ("show run\n" is one command). */
export function pastedLineCount(text: string): number {
  const normalized = text.replace(/\r\n?/g, "\n").replace(/\n$/, "");
  return normalized === "" ? 0 : normalized.split("\n").length;
}

/** Pastes of more than one line need a confirmation (a configuration sent by
 * mistake can cut access to a device). */
export function needsPasteConfirmation(text: string): boolean {
  return pastedLineCount(text) > 1;
}
