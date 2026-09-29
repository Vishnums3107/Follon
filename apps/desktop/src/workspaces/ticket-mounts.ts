// The React roots currently mounted for the order and combination tickets.

import { createRoot } from "react-dom/client";

export const mountedTickets: {
  order?: ReturnType<typeof createRoot>;
  combo?: ReturnType<typeof createRoot>;
} = {};
