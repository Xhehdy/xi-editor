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

app = FastAPI(title="Cortex Sidecar", version="0.1.0")

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
llm = ChatOpenAI(model="gpt-4-turbo-preview")

@app.get("/health")
async def health_check():
    return {"status": "ok"}

@app.post("/embeddings", response_model=EmbeddingResponse)
async def generate_embedding(request: EmbeddingRequest):
    try:
        vector = embeddings.embed_query(request.text)
        return EmbeddingResponse(vector=vector)
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/completion", response_model=CompletionResponse)
async def generate_completion(request: CompletionRequest):
    try:
        # Simple RAG-like chain
        template = """You are an AI assistant in the Cortex IDE.
        
        Context:
        {context}
        
        User Request:
        {prompt}
        """
        
        prompt = ChatPromptTemplate.from_template(template)
        chain = prompt | llm | StrOutputParser()
        
        response = chain.invoke({
            "context": request.context or "No context provided.",
            "prompt": request.prompt
        })
        
        return CompletionResponse(text=response)
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

if __name__ == "__main__":
    port = int(os.getenv("SIDECAR_PORT", 8000))
    uvicorn.run(app, host="127.0.0.1", port=port)
