/** Arrow keys move between the new-record row's inputs, like a spreadsheet. */
export function navigateDraft(event: React.KeyboardEvent<HTMLInputElement>, index: number) {
  const input = event.currentTarget;
  let next = index;
  if (event.key === "ArrowRight" && input.selectionStart === input.value.length) next = index + 1;
  if (event.key === "ArrowLeft" && input.selectionStart === 0) next = index - 1;
  if (event.key === "ArrowDown") next = index + 1;
  if (event.key === "ArrowUp") next = index - 1;
  if (next === index) return;
  const target = document.querySelector<HTMLInputElement>(`[data-draft-index="${next}"]`);
  if (target) {
    event.preventDefault();
    target.focus();
    target.select();
  }
}
