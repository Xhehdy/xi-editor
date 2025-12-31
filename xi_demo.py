import subprocess
import json
import sys
import os
import time

def main():
    print("🚀 Starting Xi Core...")
    # Ensure we are in the right directory to find the rust folder
    base_dir = os.path.dirname(os.path.abspath(__file__))
    rust_dir = os.path.join(base_dir, "rust")
    
    process = subprocess.Popen(
        ["cargo", "run", "--quiet"],
        cwd=rust_dir,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1
    )

    def send(method, params, id=None):
        msg_dict = {"method": method, "params": params}
        if id is not None:
            msg_dict["id"] = id
        msg = json.dumps(msg_dict)
        print(f"\033[94m[Client -> Core]\033[0m {msg}")
        process.stdin.write(msg + "\n")
        process.stdin.flush()

    def receive(timeout=2):
        # A simple way to read with timeout
        import select
        ready, _, _ = select.select([process.stdout], [], [], timeout)
        if ready:
            line = process.stdout.readline()
            if line:
                print(f"\033[92m[Core -> Client]\033[0m {line.strip()}")
                try:
                    return json.loads(line)
                except json.JSONDecodeError:
                    return None
        return None

    try:
        # Step 1: Initialize
        send("client_started", {"config_dir": "/tmp/xi-test-config"})
        
        # Core sends available_languages and available_themes
        for _ in range(5): # Try to catch initial announcements
            receive(0.5)

        # Step 2: Create a new view
        print("\n📝 Creating a new view...")
        send("new_view", {"file_path": None}, id=1)
        
        # Wait for the view_id response
        view_id = None
        start_time = time.time()
        while time.time() - start_time < 5:
            resp = receive(1)
            if resp and resp.get('id') == 1:
                view_id = resp.get('result')
                break
        
        if not view_id:
            print("❌ Failed to get view_id")
            return

        print(f"✅ View created with ID: {view_id}")

        # Step 3: Insert some text
        print("\n⌨️ Inserting text: 'Hello from Trae!'")
        send("edit", {
            "method": "insert",
            "params": {"chars": "Hello from Trae!"},
            "view_id": view_id
        })
        
        # Core sends update
        receive(1)

        print("\n✨ Success! Xi Core is running and responding to RPC.")
        print("This confirms the backend is fully functional and ready for a frontend.")

    except Exception as e:
        print(f"Error: {e}")
    finally:
        process.terminate()

if __name__ == "__main__":
    main()
