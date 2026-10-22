import { redirect } from "next/navigation";
import { adminFromPage } from "@/lib/auth";
import LoginForm from "@/components/login-form";

export const dynamic = "force-dynamic";

export default async function Login() {
  if (await adminFromPage()) redirect("/");
  return <LoginForm />;
}
