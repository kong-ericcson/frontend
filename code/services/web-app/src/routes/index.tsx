import { useEffect, useState } from "react";
import { createFileRoute } from "@tanstack/react-router";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

export const Route = createFileRoute("/")({
  head: () => ({
    meta: [
      { title: "Bluetext Cloud" },
      { name: "description", content: "Bluetext web application" },
    ],
  }),
  component: Home,
});

const API_BASE = import.meta.env.VITE_API_URL ?? "/api";

function Home() {
  const [apiMessage, setApiMessage] = useState<string>("Loading...");
  const [apiConnected, setApiConnected] = useState<boolean | null>(null);

  useEffect(() => {
    fetch(`${API_BASE}/hello`)
      .then((res) => res.json())
      .then((data) => {
        setApiMessage(data.message);
        setApiConnected(true);
      })
      .catch(() => {
        setApiMessage("Failed to reach API");
        setApiConnected(false);
      });
  }, []);

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-50 dark:bg-gray-900 p-4">
      <Card className="max-w-2xl w-full">
        <CardHeader className="text-center">
          <CardTitle className="text-3xl font-bold mb-2">hello from web-app</CardTitle>
          <CardDescription className="text-lg">
            {apiConnected === null && "Connecting to API..."}
            {apiConnected === true && apiMessage}
            {apiConnected === false && <span className="text-red-500">{apiMessage}</span>}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="text-center space-y-4">
            <p className="text-gray-600 dark:text-gray-400">
              This template includes everything you need to start building:
            </p>
            <div className="flex flex-wrap gap-2 justify-center">
              <Badge variant="secondary">React 19</Badge>
              <Badge variant="secondary">TanStack Start</Badge>
              <Badge variant="secondary">shadcn/ui Components</Badge>
              <Badge variant="secondary">Tailwind CSS</Badge>
              <Badge variant="secondary">TypeScript</Badge>
            </div>
          </div>

          <div className="text-center">
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Start building your application by editing the routes in the{" "}
              <code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">src/routes/</code>{" "}
              directory.
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
