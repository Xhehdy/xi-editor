import asyncio
import logging
import os
import uvicorn
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import List, Optional
from langchain_openai import OpenAIEmbeddings, ChatOpenAI
from langchain_core.prompts import ChatPromptTemplate
from langchain_core.output_parsers import StrOutputParser
from dotenv import load_dotenv

load_dotenv()
logger = logging.getLogger("glyph-sidecar")

app = FastAPI(title="Glyph Sidecar", version="0.1.0")

# Models
class EmbeddingRequest(BaseModel):
    text: str

class EmbeddingResponse(BaseModel):
    vector: List[float]

class CompletionRequest(BaseModel):
    prompt: str
    context: Optional[str] = None

class CompletionResponse(BaseModel):
    text: str

# Initialize AI components
# Ensure OPENAI_API_KEY is set
embeddings = OpenAIEmbeddings()
model_name = os.getenv("GLYPH_OPENAI_MODEL", "gpt-4o-mini")
llm = ChatOpenAI(model=model_name)

@app.get("/health")
async def health_check():
    return {"status": "ok"}

@app.post("/embeddings", response_model=EmbeddingResponse)
async def generate_embedding(request: EmbeddingRequest):
    try:
        vector = await asyncio.to_thread(embeddings.embed_query, request.text)
        return EmbeddingResponse(vector=vector)
    except Exception:
        logger.exception("embedding generation failed")
        raise HTTPException(status_code=500, detail="Embedding generation failed")

@app.post("/completion", response_model=CompletionResponse)
async def generate_completion(request: CompletionRequest):
    try:
        # Simple RAG-like chain
        template = """You are an AI assistant in the Glyph IDE.
        
        Context:
        {context}
        
        User Request:
        {prompt}
        """
        
        prompt = ChatPromptTemplate.from_template(template)
        chain = prompt | llm | StrOutputParser()
        
        response = await asyncio.to_thread(
            chain.invoke,
            {
                "context": request.context or "No context provided.",
                "prompt": request.prompt,
            },
        )
        
        return CompletionResponse(text=response)
    except Exception:
        logger.exception("completion generation failed")
        raise HTTPException(status_code=500, detail="Completion generation failed")

if __name__ == "__main__":
    port = int(os.getenv("SIDECAR_PORT", 8000))
    uvicorn.run(app, host="127.0.0.1", port=port)
