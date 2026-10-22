import { redirect } from "next/navigation";
import { adminFromPage } from "@/lib/auth";
import Dashboard from "@/components/dashboard";

export const dynamic = "force-dynamic";

export default async function Home() {
  if (!(await adminFromPage())) redirect("/login");
  return <Dashboard />;
}
