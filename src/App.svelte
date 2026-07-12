<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Button } from "$lib/components/ui/button";
  import * as Card from "$lib/components/ui/card";
  import { Input } from "$lib/components/ui/input";

  let name = $state("");
  let message = $state("Rust is ready when you are.");
  let isGreeting = $state(false);

  async function greet(event: SubmitEvent) {
    event.preventDefault();
    isGreeting = true;

    try {
      message = await invoke<string>("greet", { name });
    } catch (error) {
      message = `Could not reach Rust: ${String(error)}`;
    } finally {
      isGreeting = false;
    }
  }
</script>

<svelte:head>
  <meta
    name="description"
    content="Cepa — a desktop foundation built with Tauri, Rust, Svelte, and shadcn-svelte."
  />
</svelte:head>

<main class="flex min-h-screen items-center justify-center bg-muted/40 p-6">
  <div class="w-full max-w-sm">
    <Card.Root>
      <Card.Header>
        <Card.Title>Cepa</Card.Title>
        <Card.Description>
          Test the connection between Svelte and Rust.
        </Card.Description>
      </Card.Header>
      <Card.Content>
        <form class="space-y-4" onsubmit={greet}>
          <div class="space-y-2">
            <label class="text-sm font-medium" for="name">Name</label>
            <Input
              id="name"
              name="name"
              autocomplete="name"
              placeholder="Enter your name"
              bind:value={name}
            />
          </div>
          <Button type="submit" disabled={isGreeting}>
            {isGreeting ? "Calling Rust…" : "Say hello"}
          </Button>
        </form>
      </Card.Content>
      <Card.Footer aria-live="polite">
        <p class="text-sm text-muted-foreground">{message}</p>
      </Card.Footer>
    </Card.Root>
  </div>
</main>
