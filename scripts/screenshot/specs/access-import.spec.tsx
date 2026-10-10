import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it } from "vitest";
import App from "../../../src/App";
import { packTemplate } from "../../../tests/integration/access-template";
import { captureDocument } from "../capture";
import { dialogMock, LONG, tempPath } from "./fixtures";

it("imports an Access template with its forms, report, and data", async () => {
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: /Import Access database/ }, LONG));
  const dialog = await screen.findByRole("dialog", { name: "Import Access database" });
  dialogMock.open.mockResolvedValueOnce(packTemplate(tempPath("Order Desk.accdt")));
  await user.click(within(dialog).getByRole("button", { name: /Choose file/ }));
  const tables = await within(dialog).findByRole("table", { name: "Access tables" }, LONG);
  expect(within(tables).getByRole("cell", { name: "Customers" })).toBeInTheDocument();
  await captureDocument(document, {
    name: "access-01-inventory",
    expectations: [
      "The Import Access database dialog names the file as an Access template.",
      "It lists the Customers and Orders tables with field and row counts, and counts of queries, forms, reports, macros, and modules.",
      "The Import the data option is checked.",
    ],
  });

  await user.click(within(dialog).getByRole("button", { name: "Import" }));
  const report = await within(dialog).findByRole("region", { name: "Access import report" }, LONG);
  await user.click(within(report).getByText(/What did not convert fully/));
  expect(within(report).getByRole("table", { name: "Conversion notes" })).toBeVisible();
  await captureDocument(document, {
    name: "access-02-report",
    expectations: [
      "The report says how many tables and rows the new document has.",
      "A table counts converted, partly converted, and not converted objects per kind.",
      "The expanded notes explain each loss, such as the VBA module kept as an asset.",
    ],
  });

  await user.click(within(dialog).getByRole("button", { name: "Open document" }));
  const nav = await screen.findByRole("navigation", { name: "Application navigation" }, LONG);
  await user.click(within(nav).getByRole("button", { name: /Customer List/ }));
  const page = screen.getByRole("region", { name: "Application page" });
  await waitFor(() => expect(within(page).queryByText(/Loading/)).toBeNull(), LONG);
  await within(page).findAllByRole("row", { name: /^Open / }, LONG);
  await captureDocument(document, {
    name: "access-03-runtime",
    expectations: [
      "The imported Order Desk app opens in the Runtime with navigation built from the Access forms and report.",
      "The Customer List page shows the template's sample customers.",
    ],
  });

  await user.click(screen.getByRole("button", { name: "Settings" }));
  await user.click(await screen.findByRole("tab", { name: "Assets" }, LONG));
  await user.click(await screen.findByRole("button", { name: "View Access VBA.txt" }, LONG));
  const preview = await screen.findByRole("region", { name: "Preview of Access VBA.txt" }, LONG);
  await within(preview).findByRole("heading", { name: "Module Helpers" }, LONG);
  await captureDocument(document, {
    name: "access-04-vba",
    expectations: [
      "Settings › Assets lists the Access VBA.txt asset with a View button.",
      "The preview shows the VBA one section per module and form, with section links above.",
      "The code keeps its line breaks and indentation in a monospace block.",
    ],
  });

  await user.click(screen.getByRole("button", { name: "Query" }));
  const list = await screen.findByRole("navigation", { name: "Saved queries" }, LONG);
  await user.click(within(list).getByRole("button", { name: /^RaiseBigOrders/ }));
  await user.type(await screen.findByRole("textbox", { name: "Value for minimum" }, LONG), "0");
  await user.click(screen.getByRole("button", { name: "Preview changes" }));
  await screen.findByText(/Would update \d+ rows? in Orders/, {}, LONG);
  await captureDocument(document, {
    name: "access-05-action-query",
    expectations: [
      "The imported Access update query opens in Query mode as an action query (Update rows on Orders); action tags in the list sit inside their rows.",
      "The minimum parameter declared in Access is a typed query parameter with a value to run with.",
      "Preview changes reports how many rows it would update, and that nothing was changed.",
    ],
  });
});
