import { notFound } from "next/navigation";

// Any other channel sub-path is the shared channel 404.
export default function Unknown() {
  notFound();
}
